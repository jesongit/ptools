package net.posase.ptools.downloader;

import lombok.Data;
import lombok.EqualsAndHashCode;
import lombok.NonNull;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.*;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.concurrent.atomic.AtomicInteger;

import static net.posase.ptools.downloader.DownloadTask.BLOCK_SIZE;
import static net.posase.ptools.downloader.DownloadTask.RETRY_CNT;

@Data
@EqualsAndHashCode(callSuper = true)
public class DownloadThread extends Thread{

    Logger logger = LoggerFactory.getLogger(getClass());
    private final static int TIMEOUT = 5000;

    @NonNull private DownloadTask task;
    @NonNull private int uid;
    @NonNull private long start, end;

    private OutputStream out = null;

    @Override
    public void run() {
        AtomicInteger aliveThreads = task.getAliveThreads();
        aliveThreads.incrementAndGet();

        String fileName = task.getFile().getName();
        logger.info(String.format("Thread %d start download %s", uid, fileName));
        boolean res = downloadRange(0);
        logger.info(String.format("Thread %d download %s res %b", uid, fileName, res));

        aliveThreads.decrementAndGet();
    }

    private boolean downloadRange(int tryCount){
        URL url = task.getUrl();
        File file = task.getFile();
        String tmpPath = String.format("%s.%d.tmp", file.getAbsolutePath(), uid);

        logger.info(String.format("""
                try download url %s
                id %d start %d end %d tryCount %d
                tmpPath %s""", url, uid, start, end, tryCount, tmpPath));

        try (OutputStream out = new FileOutputStream(tmpPath)) {
//
            HttpURLConnection con = (HttpURLConnection) url.openConnection();
            con.setRequestProperty("Range", String.format("bytes=%d-%d", start, end));
            con.setConnectTimeout(TIMEOUT);
            con.setReadTimeout(TIMEOUT);
            con.connect();

            int partSize = con.getHeaderFieldInt("Content-Length", -1);
            if (partSize != end - start + 1) return false;

            try (InputStream in = con.getInputStream()) {
                byte[] buffer = new byte[BLOCK_SIZE];
                int size;
                while (start <= end && (size = in.read(buffer)) > 0) {
                    start += size;
                    task.getDownloadSize().addAndGet(size);
                    out.write(buffer, 0, size);
                    out.flush();
                }
                if (start <= end) return false;
            }
            con.disconnect();
            return true;
        } catch (Exception e) {
            logger.info(String.format("Part %d", uid + 1), e.getMessage());
        }
        if(tryCount < RETRY_CNT)
            downloadRange(tryCount + 1);
        return false;
    }

}

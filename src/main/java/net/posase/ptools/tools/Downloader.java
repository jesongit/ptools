package net.posase.ptools.tools;

import lombok.AllArgsConstructor;
import lombok.Data;
import lombok.RequiredArgsConstructor;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.*;
import java.net.ConnectException;
import java.net.HttpURLConnection;
import java.net.SocketTimeoutException;
import java.net.URL;
import java.util.concurrent.atomic.AtomicInteger;

@Data
@RequiredArgsConstructor
public class Downloader extends Thread{

    private final static Logger logger = LoggerFactory.getLogger(Downloader.class);

    private final static int MIN_SIZE = 2 << 20; // 多线程下载的最小大小
    private final static int RETRY_CNT = 5;      // 重试次数

    private AtomicInteger downloadSize = new AtomicInteger(0);
    private AtomicInteger aliveThreads = new AtomicInteger(0);

    private int id, start, end, fileSize;
    private URL url = null;
    private File file = null;
    private OutputStream out;

    public static void get(URL url, File file) throws IOException {
        get(url, file, 8, 5000);
    }

    public static void get(URL url, File file, int threadNum, int timeout) throws IOException {
        long startTime = System.currentTimeMillis();

        int fileSize = Downloader.getFileSize(url);
        boolean multithreaded = threadNum > 1 && fileSize > 0 && fileSize > MIN_SIZE;
        fileSize = Math.abs(fileSize);

        if(!multithreaded) {
            Downloader downloader = new Downloader();
        }

        if (!multithreaded) {
            new MyDownloader.DownloadThread(0, 0, fileSize - 1).start();;
        }
        else {
            endPoint = new int[THREAD_NUM + 1];
            int block = fileSize / THREAD_NUM;
            for (int i = 0; i < THREAD_NUM; i++) {
                endPoint[i] = block * i;
            }
            endPoint[THREAD_NUM] = fileSize;
            for (int i = 0; i < THREAD_NUM; i++) {
                new MyDownloader.DownloadThread(i, endPoint[i], endPoint[i + 1] - 1).start();
            }
        }

        startDownloadMonitor();

        //等待 downloadMonitor 通知下载完成
        try {
            synchronized(waiting) {
                waiting.wait();
            }
        } catch (InterruptedException e) {
            System.err.println("Download interrupted.");
        }

        cleanTempFile();

        long timeElapsed = System.currentTimeMillis() - startTime;
        System.out.println("* File successfully downloaded.");
        System.out.println(String.format("* Time used: %.3f s, Average speed: %d KB/s",
                timeElapsed / 1000.0, downloadedBytes.get() / timeElapsed));
    }

    // 获取文件大小，正数表示支持断电续传
    public static int getFileSize(URL url) throws IOException {
        HttpURLConnection con = (HttpURLConnection) url.openConnection();
        con.setRequestProperty("Range", "bytes=0-");
        for(int i = 1; i <= RETRY_CNT; ++i) {
            try {
                con.connect();
                int fileSize = con.getContentLength();
                boolean resume = con.getResponseCode() == 206;

                logger.info(String.format("url: %s fileSize: %d resume: %b", url, fileSize, resume));
                return resume ? fileSize : -fileSize;
            } catch (ConnectException e) {
                logger.info("connect fail count " + i, e.getMessage());
            }
        }
        return 0;
    }

    @Override
    public void run() {
        boolean success = false;
        while (true) {
            success = download();
            if (success) {
                System.out.println("* Downloaded part " + (id + 1));
                break;
            } else {
                System.out.println("Retry to download part " + (id + 1));
            }
        }
        aliveThreads.decrementAndGet();
    }

    //下载文件指定范围的部分
    public boolean download() {
        try {
            HttpURLConnection con = (HttpURLConnection) url.openConnection();
            con.setRequestProperty("Range", String.format("bytes=%d-%d", start, end));
            con.setConnectTimeout(TIME_OUT);
            con.setReadTimeout(TIME_OUT);
            con.connect();
            int partSize = con.getHeaderFieldInt("Content-Length", -1);
            if (partSize != end - start + 1) return false;
            if (out == null) out = new FileOutputStream(localFile.getAbsolutePath() + "." + id + ".tmp");
            try (InputStream in = con.getInputStream()) {
                byte[] buffer = new byte[1024];
                int size;
                while (start <= end && (size = in.read(buffer)) > 0) {
                    start += size;
                    downloadedBytes.addAndGet(size);
                    out.write(buffer, 0, size);
                    out.flush();
                }
                con.disconnect();
                if (start <= end) return false;
                else out.close();
            }
        } catch(SocketTimeoutException e) {
            System.out.println("Part " + (id + 1) + " Reading timeout.");
            return false;
        } catch (IOException e) {
            System.out.println("Part " + (id + 1) + " encountered error.");
            return false;
        }

        return true;
    }
}

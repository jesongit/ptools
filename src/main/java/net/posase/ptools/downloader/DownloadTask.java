package net.posase.ptools.downloader;

import lombok.Data;
import lombok.NonNull;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.*;
import java.net.ConnectException;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.concurrent.atomic.AtomicInteger;

@Data
public class DownloadTask {

    Logger logger = LoggerFactory.getLogger(getClass());
    private final static int MIN_SIZE = 2 << 18, THREAD_NUM = 8, TIMEOUT = 5000;
    final static int BLOCK_SIZE = 1024 * 100, RETRY_CNT = 10;
    @NonNull private URL url;
    @NonNull private File file;

    private AtomicInteger downloadSize = new AtomicInteger(0), aliveThreads = new AtomicInteger(0);
    private long fileSize = 0L;

    private long startTime, closeTime;
    private boolean resume = false; // todo

    private final Object display = new Object();

    public void download() {
        startTime = System.currentTimeMillis();

        fileSize = Math.abs(getFileSize());

        logger.info(String.format("resume: %b fileSize: %d MIN_SIZE:  %d", resume, fileSize, MIN_SIZE));
        boolean multiDownload = fileSize >= MIN_SIZE;
        if(multiDownload) {

            // 多线程下载
            long block = fileSize / THREAD_NUM;
            for (int i = 0; i < THREAD_NUM - 1; i++) {
                long start = block * i;
                new DownloadThread(this, i, start, start + block - 1).start();
            }
            // 最后一个要算到文件结尾
            int id = THREAD_NUM - 1;
            long start = block * id;
            new DownloadThread(this, id, start, fileSize - 1).start();
        } else {
            // 单线程下载
            new DownloadThread(this, 0, 0, fileSize -1).start();
        }

        displayDownload();

        closeTime = System.currentTimeMillis();
        tryMergeFile(multiDownload);
        double useTime = closeTime - startTime, mergeTime = (System.currentTimeMillis() - closeTime) / 1000.0;
        logger.info(String.format("""
                download Success.
                FileName: %s
                FileSize: %d
                DLTime: %.3f s
                AvgSpeed: %.3f KB/s
                MergeTime: %.3f s
                """, file.getAbsolutePath(), fileSize, useTime / 1000, fileSize / useTime, mergeTime));
    }

    // 获取文件大小，> 0 则可以断点续传
    private long getFileSize(){
        try {
            HttpURLConnection con = (HttpURLConnection) url.openConnection();
            con.setInstanceFollowRedirects(false);
            con.setRequestProperty("Accept-Encoding", "identity"); // 加上这个防止大文件返回-1
            con.connect();
            if(con.getResponseCode() == 302) {

                // 非直链需要重定向
                String link = con.getHeaderField("Location");
                logger.info(String.format("http302: %s", link));

                url = new URL(link);
                con = (HttpURLConnection) url.openConnection();
                con.setRequestProperty("Accept-Encoding", "identity");
                con.connect();
            }
            for(int i = 0; i < RETRY_CNT; ++i) {
                try {
                    if(i > 0) con.connect();
                    System.out.println(con.getResponseCode());
                    long fileSize = con.getContentLengthLong();
                    boolean resume = con.getResponseCode() == 206;

                    logger.info(String.format("url: %s fileSize: %d resume: %b", url, fileSize, resume));
                    return resume ? fileSize : -fileSize;
                } catch (ConnectException e) {
                    logger.info(String.format("connect fail count %d", i + 1), e.getMessage());
                }
            }
        } catch (IOException e) {
            logger.info(String.format("url: %s connect fail", url), e.getMessage());
        }
        throw new RuntimeException("fileSize == 0");
    }

    private void tryMergeFile(boolean multiDownload) {
        logger.info("start merge");
        try (OutputStream out = new FileOutputStream(file)) {
            logger.info("start merge 1");
            byte[] buffer = new byte[BLOCK_SIZE];
            int size, realThreadNum = multiDownload ? THREAD_NUM : 1;
            logger.info(multiDownload + "" + realThreadNum);
            for (int i = 0; i < realThreadNum; ++i) {
                String tmpFile = String.format("%s.%d.tmp", file.getAbsolutePath(), i);
                File f = new File(tmpFile);
                System.out.println(f.getName());
                System.out.println(f.exists());
                System.out.println(f.length());
                InputStream in = new FileInputStream(tmpFile);
                while ((size = in.read(buffer)) != -1)
                    out.write(buffer, 0, size);
                in.close();
                Files.delete(Paths.get(tmpFile));
            }
        } catch (IOException e) {
            logger.info(String.format("merge file fail multiDownload %b", multiDownload), e.getMessage());
        }
    }

    // 显示下载速度
    private void displayDownload() {

        // 打印线程
        Thread thread = new Thread(() ->{
            int pre = 0, cur, num;
            try {
                do{
                    // 每秒打印一次
                    Thread.sleep(1000);
                    num = aliveThreads.get();
                    cur = downloadSize.get();
                    logger.info(String.format("Speed: %d KB/s, Downloaded: %d KB (%.2f%%), Threads: %d",
                            (cur - pre) >> 10, cur >> 10, cur / (float) fileSize * 100, THREAD_NUM));
                    pre = cur;
                } while (num != 0);

                // 无存活线程，通知主线程
                synchronized (display) {
                    display.notifyAll();
                }
            } catch (InterruptedException e) {
                throw new RuntimeException(e);
            }
        });

        thread.setDaemon(true);
        thread.start();

        // 等待下载完成
        synchronized (display) {
            try {
                display.wait();
            } catch (InterruptedException e) {
                logger.info(String.format("Download interrupted. %s", e.getMessage()));
            }
        }
    }
}

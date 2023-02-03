package net.posase.ptools.tools;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.io.IOException;
import java.net.ConnectException;
import java.net.HttpURLConnection;
import java.net.URL;

public class Utils {

    private final int MIN_SIZE = 2 << 20; // 多线程下载的最小大小

    Logger logger = LoggerFactory.getLogger(getClass());


    public static void get(URL url, int thread_num = 8) throws IOException {
        long startTime = System.currentTimeMillis();

        int fileSize = getFileSize(url);
        boolean muldownload = thread_num > 0 && fileSize > 0 && fileSize > MIN_SIZE;
        fileSize = Math.abs(fileSize);

        AtomicInteger downloadSize = new AtomicInteger(0);
        AtomicInteger aliveThreads = new AtomicInteger(0);


        if (!resumable || THREAD_NUM == 1|| fileSize < MIN_SIZE) multithreaded = false;
        if (!multithreaded) {
            new DownloadThread(0, 0, fileSize - 1).start();;
        }
        else {
            endPoint = new int[THREAD_NUM + 1];
            int block = fileSize / THREAD_NUM;
            for (int i = 0; i < THREAD_NUM; i++) {
                endPoint[i] = block * i;
            }
            endPoint[THREAD_NUM] = fileSize;
            for (int i = 0; i < THREAD_NUM; i++) {
                new DownloadThread(i, endPoint[i], endPoint[i + 1] - 1).start();
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

    public static File download_file(File file, String link) {

        if(file.exists())
            return file;

        try {
            // todo download file and save to file
            return file;
        } catch(Exception e){
            return null;
        }
    }
}

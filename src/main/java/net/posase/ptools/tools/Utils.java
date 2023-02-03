package net.posase.ptools.tools;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.io.IOException;
import java.net.ConnectException;
import java.net.HttpURLConnection;
import java.net.URL;

public class Utils {

    Logger logger = LoggerFactory.getLogger(getClass());

    //检测目标文件是否支持断点续传，以决定是否开启多线程下载文件的不同部分
    public int getFileSize(URL url) throws IOException {
        HttpURLConnection con = (HttpURLConnection) url.openConnection();
        con.setRequestProperty("Range", "bytes=0-");
        for(int i = 1; i <= retryCount; ++i) {
            try {
                con.connect();
                int fileSize = con.getContentLength();
                if(con.getResponseCode() == 206)
                    return fileSize;
                else
                    return -fileSize;
            } catch (ConnectException e) {
                logger.info("connect fail count " + i, e.getMessage());
            }
        }
        return 0;
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

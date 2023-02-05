package net.posase.ptools.downloader;

import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.File;
import java.net.URL;

@Data
@NoArgsConstructor
public class Downloader {
    private static Downloader instance = new Downloader();

    public static Downloader getInstance() {
        return instance;
    }

    public void start(URL url, File file){
        DownloadTask task = new DownloadTask(url, file);
        task.download();
    }

}

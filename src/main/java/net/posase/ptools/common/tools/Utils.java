package net.posase.ptools.common.tools;

import net.posase.ptools.common.downloader.Downloader;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Paths;

public class Utils {

    private final static Logger logger = LoggerFactory.getLogger(Utils.class);
    public static File download_file(String path, String link) {
        try{
            File file = new File(path);
            if(file.exists())
                Files.delete(Paths.get(path));

            Downloader downloader = Downloader.getInstance();
            downloader.start(new URL(link), file);
            return file;
        } catch(Exception e){
            logger.info(String.format("download fail path: %s link: %s", path, link), e.getMessage());
            return null;
        }
    }

    public static void main(String[] args) {
        String link = "https://mirrors.tuna.tsinghua.edu.cn/centos/7.9.2009/isos/x86_64/CentOS-7-x86_64-DVD-2009.iso";
        File file = Utils.download_file("./test.msi", link);
        System.out.println(file);
    }
}

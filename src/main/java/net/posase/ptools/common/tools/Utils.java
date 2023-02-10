package net.posase.ptools.common.tools;

import cn.dev33.satoken.secure.SaSecureUtil;
import net.posase.ptools.common.downloader.DownloadCallBack;
import net.posase.ptools.common.downloader.Downloader;
import net.posase.ptools.common.exception.ApiException;
import net.posase.ptools.common.exception.ErrorCode;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Paths;

public class Utils {

    private final static Logger logger = LoggerFactory.getLogger(Utils.class);
    private final static String key = "LD4pMdPI9R0ruBK";

    public static void downloadFile(String path, String link) {
        try{
            File file = new File(path);
            if(file.exists())
                Files.delete(Paths.get(path));
            Downloader downloader = new Downloader(new URL(link), file);
            downloader.download();
        } catch(Exception e){
            logger.info(String.format("download fail path: %s link: %s", path, link), e.getMessage());
        }
    }

    public static void downloadFile(String path, String link, Downloader downloader) {
        try{
            File file = new File(path);
            if(file.exists())
                Files.delete(Paths.get(path));

            downloader.setUrl(new URL(link));
            downloader.setFile(file);
            downloader.download();
        } catch(Exception e){
            logger.info(String.format("download fail path: %s link: %s", path, link), e.getMessage());
        }
    }

    // RAS 加密
    public static String encrypt(String password) {
        return SaSecureUtil.aesEncrypt(key, password);
    }

    // RAS 解密
    public static String decrypt(String password) {
        return SaSecureUtil.aesDecrypt(key, password);
    }

    // 断言
    public static void iAssert(boolean expr, ErrorCode errorCode) {
        if(!expr) throw new ApiException(errorCode);
    }

    public static void main(String[] args) {
        String path = "./test.msi";
        String link = "https://mirrors.tuna.tsinghua.edu.cn/centos/7.9.2009/isos/x86_64/CentOS-7-x86_64-DVD-2009.iso";
//        Utils.downloadFile(path, link);
        Utils.downloadFile(path, link, new Downloader(){
            @Override
            public void finish_download() {
                System.out.println("download complete.");
            }
        });
    }
}

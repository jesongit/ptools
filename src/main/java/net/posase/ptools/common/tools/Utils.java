package net.posase.ptools.common.tools;

import cn.dev33.satoken.secure.SaSecureUtil;
import com.abercap.mediainfo.api.MediaInfo;
import net.posase.ptools.common.downloader.Downloader;
import net.posase.ptools.common.exception.ApiException;
import net.posase.ptools.common.enums.ErrorCode;
import net.posase.ptools.modules.tms.entity.Mv;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Paths;

public class Utils {

    private final static Logger logger = LoggerFactory.getLogger(Utils.class);
    private final static String key = "LD4pMdPI9R0ruBK";

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
    }
}

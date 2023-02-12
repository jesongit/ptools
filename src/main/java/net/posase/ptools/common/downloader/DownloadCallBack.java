package net.posase.ptools.common.downloader;

import java.io.File;

public interface DownloadCallBack {

    void before_download(File file);

    void finish_download(File file);
}

package net.posase.ptools.common.downloader;

import lombok.Data;
import lombok.NonNull;
import net.posase.ptools.common.enums.IState;
import net.posase.ptools.modules.tms.entity.Mv;
import net.posase.ptools.modules.tms.service.MvService;
import org.springframework.beans.factory.annotation.Autowired;

import java.io.File;

@Data
public class MvCallBack implements DownloadCallBack{

    @NonNull  private Mv mv;

    @Autowired
    private MvService mvService;

    @Override
    public void before_download(File file) {

    }

    @Override
    public void finish_download(File file) {
        mv.setPath(file.getAbsolutePath());
        mv.setState(IState.DOWNLOAD);
        mvService.save(mv);
    }
}

package net.posase.ptools.modules.tms.service.impl;

import net.posase.ptools.common.downloader.Downloader;
import net.posase.ptools.common.downloader.MvCallBack;
import net.posase.ptools.common.enums.DealType;
import net.posase.ptools.common.enums.ErrorCode;
import net.posase.ptools.common.exception.ApiException;
import net.posase.ptools.common.tools.Uploader;
import net.posase.ptools.common.tools.Utils;
import net.posase.ptools.modules.tms.entity.Mv;
import net.posase.ptools.modules.tms.mapper.MvMapper;
import net.posase.ptools.modules.tms.service.MvService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

import java.io.File;
import java.net.MalformedURLException;
import java.net.URL;
import java.util.List;

/**
 * <p>
 * MV信息表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@Service
public class MvServiceImpl extends ServiceImpl<MvMapper, Mv> implements MvService {

    @Override
    public void download(List<String> uuidList, String path) {
        List<Mv> mvList = baseMapper.selectBatchIds(uuidList);
        Utils.iAssert(mvList.size() > 0, ErrorCode.MV_VALID);

        File dir = new File(path);
        Utils.iAssert(dir.exists() && dir.isDirectory(), ErrorCode.PATH_VALID);

        for (Mv mv : mvList) {
            try {
                File file = new File(path + mv.getName());
                URL url = new URL(mv.getDownloadLink());
                new Downloader(url, file, new MvCallBack(mv));
            } catch (MalformedURLException e) {
                throw new ApiException(ErrorCode.URL_VALID);
            }
        }
    }

    @Override
    public void deal(Mv mv) {
        baseMapper.updateById(mv);
        Uploader.deal_upload(mv, DealType.QMusic);
    }
}

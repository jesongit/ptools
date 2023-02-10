package net.posase.ptools.modules.tms.service.impl;

import net.posase.ptools.common.exception.ErrorCode;
import net.posase.ptools.common.tools.Utils;
import net.posase.ptools.modules.tms.entity.Mv;
import net.posase.ptools.modules.tms.mapper.MvMapper;
import net.posase.ptools.modules.tms.service.MvService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

import java.io.File;
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
    public void download(List<String> uuid_list, String path) {
        List<Mv> list = baseMapper.selectBatchIds(uuid_list);
        Utils.iAssert(list.size() > 0, ErrorCode.MV_VALID);

        File dir = new File(path);
        Utils.iAssert(dir.isDirectory() && dir.exists(), ErrorCode.PATH_VALID);

        list.forEach(mv -> {
            // todo
        });
    }
}

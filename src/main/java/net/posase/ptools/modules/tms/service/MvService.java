package net.posase.ptools.modules.tms.service;

import net.posase.ptools.modules.tms.entity.Mv;
import com.baomidou.mybatisplus.extension.service.IService;

import java.util.List;

/**
 * <p>
 * MV信息表 服务类
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
public interface MvService extends IService<Mv> {
    void download(List<String> uuid, String path);
}

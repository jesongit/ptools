package net.posase.ptools.modules.ums.service.impl;

import net.posase.ptools.modules.ums.entity.Resource;
import net.posase.ptools.modules.ums.mapper.ResourceMapper;
import net.posase.ptools.modules.ums.service.ResourceService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 后台资源表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class ResourceServiceImpl extends ServiceImpl<ResourceMapper, Resource> implements ResourceService {

}

package net.posase.ptools.modules.ums.service.impl;

import net.posase.ptools.modules.ums.entity.Admin;
import net.posase.ptools.modules.ums.mapper.AdminMapper;
import net.posase.ptools.modules.ums.service.IAdminService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 后台用户表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class AdminServiceImpl extends ServiceImpl<AdminMapper, Admin> implements IAdminService {

}

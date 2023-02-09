package net.posase.ptools.modules.ums.service.impl;

import net.posase.ptools.modules.ums.entity.AdminLoginLog;
import net.posase.ptools.modules.ums.mapper.AdminLoginLogMapper;
import net.posase.ptools.modules.ums.service.IAdminLoginLogService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 后台用户登录日志表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class AdminLoginLogServiceImpl extends ServiceImpl<AdminLoginLogMapper, AdminLoginLog> implements IAdminLoginLogService {

}

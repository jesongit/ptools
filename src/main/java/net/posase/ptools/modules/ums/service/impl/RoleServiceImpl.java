package net.posase.ptools.modules.ums.service.impl;

import net.posase.ptools.modules.ums.entity.Role;
import net.posase.ptools.modules.ums.mapper.RoleMapper;
import net.posase.ptools.modules.ums.service.IRoleService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 后台用户角色表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class RoleServiceImpl extends ServiceImpl<RoleMapper, Role> implements IRoleService {

}

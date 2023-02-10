package net.posase.ptools.modules.ums.service;

import com.baomidou.mybatisplus.extension.service.IService;
import net.posase.ptools.modules.ums.dto.UserInfoDto;
import net.posase.ptools.modules.ums.dto.UserLoginDto;
import net.posase.ptools.modules.ums.entity.Admin;

/**
 * <p>
 * 后台用户表 服务类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
public interface AdminService extends IService<Admin> {

    Admin register(UserInfoDto userInfo);

    void login(UserLoginDto userLogin);

    void logout();

}

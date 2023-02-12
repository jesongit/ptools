package net.posase.ptools.modules.ums.service.impl;

import cn.dev33.satoken.stp.StpUtil;
import net.posase.ptools.common.enums.ErrorCode;
import net.posase.ptools.common.tools.Utils;
import net.posase.ptools.modules.ums.dto.UserInfoDto;
import net.posase.ptools.modules.ums.dto.UserLoginDto;
import net.posase.ptools.modules.ums.entity.Admin;
import net.posase.ptools.modules.ums.mapper.AdminLoginLogMapper;
import net.posase.ptools.modules.ums.mapper.AdminMapper;
import net.posase.ptools.modules.ums.service.AdminService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.beans.BeanUtils;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;

import java.util.List;

/**
 * <p>
 * 后台用户表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class AdminServiceImpl extends ServiceImpl<AdminMapper, Admin> implements AdminService {

    @Autowired
    AdminLoginLogMapper loginMapper;

    @Override
    public Admin register(UserInfoDto userInfo) {
        String username = userInfo.getUsername();

        List<Admin> res = baseMapper.selectByUsername(username);
        Utils.iAssert(res.size() == 0, ErrorCode.NAME_VALID);

        Admin admin = new Admin();
        userInfo.setPassword(Utils.encrypt(userInfo.getPassword()));
        BeanUtils.copyProperties(userInfo, admin);
        baseMapper.insert(admin);

        return admin;
    }

    @Override
    public void login(UserLoginDto userLogin) {
        String username = userLogin.getUsername();
        List<Admin> list = baseMapper.selectByUsername(username);

        Admin admin = list.get(0);
        String password = Utils.decrypt(admin.getPassword());
        Utils.iAssert(password.equals(userLogin.getPassword()), ErrorCode.PWD_VALID);

        StpUtil.login(admin.getId());
    }

    @Override
    public void logout() {
        StpUtil.logout();
    }
}

package net.posase.ptools.modules.ums.controller;

import cn.dev33.satoken.util.SaResult;
import net.posase.ptools.modules.ums.dto.UserInfoDto;
import net.posase.ptools.modules.ums.dto.UserLoginDto;
import net.posase.ptools.modules.ums.entity.Admin;
import net.posase.ptools.modules.ums.service.AdminService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.validation.annotation.Validated;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import org.springframework.web.bind.annotation.RestController;

/**
 * <p>
 * 后台用户表 前端控制器
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@RestController
@RequestMapping("/admin")
public class AdminController {

    @Autowired
    AdminService adminService;

    @RequestMapping(value = "register", method = RequestMethod.POST)
    public SaResult register(@Validated @RequestBody UserInfoDto info) {
        Admin admin = adminService.register(info);
        return SaResult.data(admin);
    }

    @RequestMapping(value = "login", method = RequestMethod.POST)
    public SaResult login(@Validated @RequestBody UserLoginDto userLogin) {
        adminService.login(userLogin);
        return SaResult.ok();
    }

    @RequestMapping(value = "logout", method = RequestMethod.POST)
    public SaResult login() {
        adminService.logout();
        return SaResult.ok();
    }
}

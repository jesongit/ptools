package net.posase.ptools.config;

import cn.dev33.satoken.stp.StpInterface;
import org.springframework.stereotype.Component;

import java.util.ArrayList;
import java.util.List;

@Component
public class SaTokenImpl implements StpInterface {

    @Override
    public List<String> getPermissionList(Object loginId, String loginType) {
        //返回一个账号的权限列表
        return new ArrayList<>();
    }

    @Override
    public List<String> getRoleList(Object loginId, String loginType) {
        //返回一个账号的角色列表
        return null;
    }

}

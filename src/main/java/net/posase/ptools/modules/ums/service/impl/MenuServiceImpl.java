package net.posase.ptools.modules.ums.service.impl;

import net.posase.ptools.modules.ums.entity.Menu;
import net.posase.ptools.modules.ums.mapper.MenuMapper;
import net.posase.ptools.modules.ums.service.IMenuService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 后台菜单表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
@Service
public class MenuServiceImpl extends ServiceImpl<MenuMapper, Menu> implements IMenuService {

}

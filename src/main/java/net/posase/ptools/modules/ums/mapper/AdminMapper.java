package net.posase.ptools.modules.ums.mapper;

import net.posase.ptools.modules.ums.entity.Admin;
import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

import java.util.List;

/**
 * <p>
 * 后台用户表 Mapper 接口
 * </p>
 *
 * @author posase
 * @since 2023-02-09
 */
public interface AdminMapper extends BaseMapper<Admin> {

    @Select("select * from ums_admin where `username`='${name}'")
    List<Admin> selectByUsername(@Param("name") String name);
}

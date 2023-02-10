package net.posase.ptools.modules.ums.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import java.io.Serializable;
import io.swagger.v3.oas.annotations.media.Schema;

/**
 * <p>
 * 后台用户和角色关系表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@TableName("ums_admin_role_relation")
@Schema(name = "AdminRoleRelation", description = "$!{table.comment}")
public class AdminRoleRelation implements Serializable {

    private static final long serialVersionUID = 1L;

    @TableId(value = "uid", type = IdType.AUTO)
    private Long uid;

    @Schema(description = "用户id")
    private Long id;

    @Schema(description = "角色id")
    private Long roleId;

    public Long getUid() {
        return uid;
    }

    public void setUid(Long uid) {
        this.uid = uid;
    }

    public Long getId() {
        return id;
    }

    public void setId(Long id) {
        this.id = id;
    }

    public Long getRoleId() {
        return roleId;
    }

    public void setRoleId(Long roleId) {
        this.roleId = roleId;
    }

    @Override
    public String toString() {
        return "AdminRoleRelation{" +
            "uid = " + uid +
            ", id = " + id +
            ", roleId = " + roleId +
        "}";
    }
}

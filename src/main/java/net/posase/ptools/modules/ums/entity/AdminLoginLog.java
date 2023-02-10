package net.posase.ptools.modules.ums.entity;

import com.baomidou.mybatisplus.annotation.FieldFill;
import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import java.io.Serializable;
import java.time.LocalDateTime;
import io.swagger.v3.oas.annotations.media.Schema;

/**
 * <p>
 * 后台用户登录日志表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@TableName("ums_admin_login_log")
@Schema(name = "AdminLoginLog", description = "$!{table.comment}")
public class AdminLoginLog implements Serializable {

    private static final long serialVersionUID = 1L;

    @TableId(value = "uid", type = IdType.AUTO)
    private Long uid;

    @Schema(description = "用户id")
    private Long id;

    @Schema(description = "创建时间")
    @TableField(fill = FieldFill.INSERT)
    private LocalDateTime createTime;

    @Schema(description = "ip 地址")
    private String ip;

    @Schema(description = "地址")
    private String address;

    @Schema(description = "浏览器登录类型")
    private String userAgent;

    @Schema(description = "状态码 1 成功 0 失败")
    private Integer code;

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

    public LocalDateTime getCreateTime() {
        return createTime;
    }

    public void setCreateTime(LocalDateTime createTime) {
        this.createTime = createTime;
    }

    public String getIp() {
        return ip;
    }

    public void setIp(String ip) {
        this.ip = ip;
    }

    public String getAddress() {
        return address;
    }

    public void setAddress(String address) {
        this.address = address;
    }

    public String getUserAgent() {
        return userAgent;
    }

    public void setUserAgent(String userAgent) {
        this.userAgent = userAgent;
    }

    public Integer getCode() {
        return code;
    }

    public void setCode(Integer code) {
        this.code = code;
    }

    @Override
    public String toString() {
        return "AdminLoginLog{" +
            "uid = " + uid +
            ", id = " + id +
            ", createTime = " + createTime +
            ", ip = " + ip +
            ", address = " + address +
            ", userAgent = " + userAgent +
            ", code = " + code +
        "}";
    }
}

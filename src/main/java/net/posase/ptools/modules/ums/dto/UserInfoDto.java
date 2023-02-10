package net.posase.ptools.modules.ums.dto;

import io.swagger.v3.oas.annotations.media.Schema;
import jakarta.validation.constraints.Email;
import jakarta.validation.constraints.NotEmpty;
import lombok.Getter;
import lombok.Setter;

@Getter
@Setter
public class UserInfoDto {

    @NotEmpty
    @Schema(description = "用户名")
    private String username;

    @NotEmpty
    @Schema(description = "密码")
    private String password;

    @NotEmpty
    @Schema(description = "头像")
    private String icon;

    @Email
    @Schema(description = "邮箱")
    private String email;

    @NotEmpty
    @Schema(description = "昵称")
    private String nickName;
}

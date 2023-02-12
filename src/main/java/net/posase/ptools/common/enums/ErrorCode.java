package net.posase.ptools.common.enums;

import lombok.AllArgsConstructor;
import lombok.Getter;

@Getter
@AllArgsConstructor
public enum ErrorCode {
    NAME_VALID(1001, "用户名验证失败"),
    PWD_VALID(1001, "密码验证失败"),

    MV_VALID(2001, "MV不存在"),

    PATH_VALID(9001, "路径错误"),
    URL_VALID(9002, "URL访问失败"),
    ;

    private long code;
    private String message;
}

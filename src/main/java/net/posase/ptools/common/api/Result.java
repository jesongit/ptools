package net.posase.ptools.common.api;

import lombok.AllArgsConstructor;
import lombok.Data;

/**
 * API 返回结果
 * @param <T>
 */
@Data
@AllArgsConstructor
public class Result<T> {

    private int code;
    private String message;
    private T data;

    /**
     * 成功
     */
    public static <T> Result<T> success(T data) {
        return new Result<T>(ResultCode.SUCCESS.getCode(), ResultCode.SUCCESS.getMessage(), data);
    }

    public static <T> Result<T> success(T data, String message) {
        return new Result<T>(ResultCode.SUCCESS.getCode(), message, data);
    }


    public static <T> Result<T> failed(IErrorCode errorCode,String message) {
        return new Result<T>(errorCode.getCode(), message, null);
    }

    public static <T> Result<T> failed(String message) {
        return new Result<T>(ResultCode.FAILED.getCode(), message, null);
    }

}

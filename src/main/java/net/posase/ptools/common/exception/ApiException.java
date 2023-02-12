package net.posase.ptools.common.exception;

import lombok.Getter;
import net.posase.ptools.common.enums.ErrorCode;

@Getter
public class ApiException extends RuntimeException {
    private ErrorCode errorCode;

    public ApiException(ErrorCode errorCode) {
        super(errorCode.getMessage());
        this.errorCode = errorCode;
    }

    public String getMessage() {
        return errorCode.getMessage();
    }
}

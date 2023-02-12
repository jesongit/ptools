package net.posase.ptools.common.enums;

import com.baomidou.mybatisplus.annotation.IEnum;
import lombok.AllArgsConstructor;

@AllArgsConstructor
public enum IState implements IEnum<Integer> {

    DEFAULT(0, "默认"),
    DOWNLOAD(1, "下载完成"),
    UPLOAD(2, "上传完成");

    private int value;
    private String message;

    @Override
    public Integer getValue() {
        return null;
    }
}

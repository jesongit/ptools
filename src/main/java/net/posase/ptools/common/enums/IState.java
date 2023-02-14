package net.posase.ptools.common.enums;

import com.baomidou.mybatisplus.annotation.IEnum;
import lombok.AllArgsConstructor;

@AllArgsConstructor
public enum IState implements IEnum<Integer> {

    DEFAULT(0, "默认"),
    RENAME(1, "重命名完成"),
    DOWNLOAD(2, "下载完成"),
    UPLOAD(3, "上传完成");

    private int value;
    private String message;

    @Override
    public Integer getValue() {
        return this.value;
    }
}

package net.posase.ptools.modules.tms.entity;

import com.baomidou.mybatisplus.annotation.*;

import java.io.Serializable;
import java.time.LocalDateTime;
import io.swagger.v3.oas.annotations.media.Schema;
import jakarta.validation.constraints.NotEmpty;

/**
 * <p>
 * MV信息表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@TableName("tms_mv_info")
@Schema(name = "Mv", description = "$!{table.comment}")
public class Mv implements Serializable {

    private static final long serialVersionUID = 1L;

    @Schema(description = "uuid")
    @TableId(value = "uuid", type = IdType.ASSIGN_UUID)
    private String uuid;

    @Schema(description = "hash")
    private String hash;

    @NotEmpty
    @Schema(description = "文件名")
    private String name;

    @Schema(description = "相对路径")
    private String path;

    @Schema(description = "分辨率宽")
    private Integer width;

    @Schema(description = "分辨率高")
    private Integer height;

    @Schema(description = "版本号")
    private Integer version;

    @Schema(description = "音频格式")
    private String audioFormat;

    @Schema(description = "视频格式")
    private String videoFormat;

    @Schema(description = "详情链接")
    private String infoLink;

    @NotEmpty
    @Schema(description = "源链接")
    private String downloadLink;

    @TableField(fill = FieldFill.INSERT)
    private LocalDateTime createTime;

    public String getUuid() {
        return uuid;
    }

    public void setUuid(String uuid) {
        this.uuid = uuid;
    }

    public String getHash() {
        return hash;
    }

    public void setHash(String hash) {
        this.hash = hash;
    }

    public String getName() {
        return name;
    }

    public void setName(String name) {
        this.name = name;
    }

    public String getPath() {
        return path;
    }

    public void setPath(String path) {
        this.path = path;
    }

    public Integer getWidth() {
        return width;
    }

    public void setWidth(Integer width) {
        this.width = width;
    }

    public Integer getHeight() {
        return height;
    }

    public void setHeight(Integer height) {
        this.height = height;
    }

    public Integer getVersion() {
        return version;
    }

    public void setVersion(Integer version) {
        this.version = version;
    }

    public String getAudioFormat() {
        return audioFormat;
    }

    public void setAudioFormat(String audioFormat) {
        this.audioFormat = audioFormat;
    }

    public String getVideoFormat() {
        return videoFormat;
    }

    public void setVideoFormat(String videoFormat) {
        this.videoFormat = videoFormat;
    }

    public String getInfoLink() {
        return infoLink;
    }

    public void setInfoLink(String infoLink) {
        this.infoLink = infoLink;
    }

    public String getDownloadLink() {
        return downloadLink;
    }

    public void setDownloadLink(String downloadLink) {
        this.downloadLink = downloadLink;
    }

    public LocalDateTime getCreateTime() {
        return createTime;
    }

    public void setCreateTime(LocalDateTime createTime) {
        this.createTime = createTime;
    }

    @Override
    public String toString() {
        return "Mv{" +
            "uuid = " + uuid +
            ", hash = " + hash +
            ", name = " + name +
            ", path = " + path +
            ", width = " + width +
            ", height = " + height +
            ", version = " + version +
            ", audioFormat = " + audioFormat +
            ", videoFormat = " + videoFormat +
            ", infoLink = " + infoLink +
            ", downloadLink = " + downloadLink +
            ", createTime = " + createTime +
        "}";
    }
}

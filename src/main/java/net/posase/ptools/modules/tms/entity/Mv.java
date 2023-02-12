package net.posase.ptools.modules.tms.entity;

import com.baomidou.mybatisplus.annotation.*;

import java.io.Serializable;
import java.time.LocalDateTime;
import io.swagger.v3.oas.annotations.media.Schema;
import jakarta.validation.constraints.Null;
import lombok.Data;
import net.posase.ptools.common.enums.IState;

/**
 * <p>
 * MV信息表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */

@Data
@TableName("tms_mv_info")
@Schema(name = "Mv", description = "$!{table.comment}")
public class Mv implements Serializable {

    private static final long serialVersionUID = 1L;

    @Null
    @Schema(description = "uuid")
    @TableId(value = "uuid", type = IdType.ASSIGN_UUID)
    private String uuid;

    @Schema(description = "hash")
    private String hash;

    @Schema(description = "文件名")
    private String name;

    @Schema(description = "相对路径")
    private String path;

    @Schema(description = "文件状态")
    private IState state;

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

    @Schema(description = "源链接")
    private String downloadLink;

    @Null
    @TableField(fill = FieldFill.INSERT)
    private LocalDateTime createTime;
}

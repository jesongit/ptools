package net.posase.ptools.modules.tms.entity;

import com.baomidou.mybatisplus.annotation.*;

import java.io.Serializable;
import java.time.LocalDateTime;
import io.swagger.v3.oas.annotations.media.Schema;
import lombok.Data;
import net.posase.ptools.common.enums.IState;

/**
 * <p>
 * 种子信息表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */

@Data
@TableName("tms_torrent_info")
@Schema(name = "Torrent", description = "$!{table.comment}")
public class Torrent implements Serializable {

    private static final long serialVersionUID = 1L;

    @Schema(description = "种子id")
    @TableId(value = "uuid", type = IdType.INPUT)
    private Long torrentId;

    @Schema(description = "uuid")
    private String uuid;

    @Schema(description = "发布用户")
    private Long userId;

    @Schema(description = "分类")
    private Integer type;

    @Schema(description = "标题")
    private String title;

    @Schema(description = "种子状态")
    private IState state;

    @Schema(description = "副标题")
    private String subTitle;

    @Schema(description = "发布员")
    private String uploader;

    @Schema(description = "描述")
    private String descr;

    @Schema(description = "视频编码")
    private Integer videoEncode;

    @Schema(description = "音频编码")
    private Integer audioEncode;

    @Schema(description = "地区分类")
    private Integer region;

    @Schema(description = "编码版本")
    private Integer edition;

    @Schema(description = "团队小组")
    private Integer team;

    @Schema(description = "媒介")
    private Integer medium;

    @Schema(description = "标记列表(按位记录)")
    private Integer tagList;

    @Schema(description = "封面链接")
    private String coverLink;

    @Schema(description = "缩略图链接")
    private String thumbLink;

    @Schema(description = "豆瓣链接")
    private String doubanLink;

    @Schema(description = "来源")
    private String fromUrl;

    @TableField(fill = FieldFill.INSERT)
    private LocalDateTime createTime;
}

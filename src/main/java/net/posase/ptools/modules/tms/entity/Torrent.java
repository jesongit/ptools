package net.posase.ptools.modules.tms.entity;

import com.baomidou.mybatisplus.annotation.FieldFill;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableName;
import java.io.Serializable;
import java.time.LocalDateTime;
import io.swagger.v3.oas.annotations.media.Schema;

/**
 * <p>
 * 种子信息表
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@TableName("tms_torrent_info")
@Schema(name = "Torrent", description = "$!{table.comment}")
public class Torrent implements Serializable {

    private static final long serialVersionUID = 1L;

    @Schema(description = "种子id")
    private Long torrentId;

    @Schema(description = "uuid")
    private String uuid;

    @Schema(description = "发布用户")
    private Long userId;

    @Schema(description = "分类")
    private Integer type;

    @Schema(description = "标题")
    private String title;

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

    public Long getTorrentId() {
        return torrentId;
    }

    public void setTorrentId(Long torrentId) {
        this.torrentId = torrentId;
    }

    public String getUuid() {
        return uuid;
    }

    public void setUuid(String uuid) {
        this.uuid = uuid;
    }

    public Long getUserId() {
        return userId;
    }

    public void setUserId(Long userId) {
        this.userId = userId;
    }

    public Integer getType() {
        return type;
    }

    public void setType(Integer type) {
        this.type = type;
    }

    public String getTitle() {
        return title;
    }

    public void setTitle(String title) {
        this.title = title;
    }

    public String getSubTitle() {
        return subTitle;
    }

    public void setSubTitle(String subTitle) {
        this.subTitle = subTitle;
    }

    public String getUploader() {
        return uploader;
    }

    public void setUploader(String uploader) {
        this.uploader = uploader;
    }

    public String getDescr() {
        return descr;
    }

    public void setDescr(String descr) {
        this.descr = descr;
    }

    public Integer getVideoEncode() {
        return videoEncode;
    }

    public void setVideoEncode(Integer videoEncode) {
        this.videoEncode = videoEncode;
    }

    public Integer getAudioEncode() {
        return audioEncode;
    }

    public void setAudioEncode(Integer audioEncode) {
        this.audioEncode = audioEncode;
    }

    public Integer getRegion() {
        return region;
    }

    public void setRegion(Integer region) {
        this.region = region;
    }

    public Integer getEdition() {
        return edition;
    }

    public void setEdition(Integer edition) {
        this.edition = edition;
    }

    public Integer getTeam() {
        return team;
    }

    public void setTeam(Integer team) {
        this.team = team;
    }

    public Integer getMedium() {
        return medium;
    }

    public void setMedium(Integer medium) {
        this.medium = medium;
    }

    public Integer getTagList() {
        return tagList;
    }

    public void setTagList(Integer tagList) {
        this.tagList = tagList;
    }

    public String getCoverLink() {
        return coverLink;
    }

    public void setCoverLink(String coverLink) {
        this.coverLink = coverLink;
    }

    public String getThumbLink() {
        return thumbLink;
    }

    public void setThumbLink(String thumbLink) {
        this.thumbLink = thumbLink;
    }

    public String getDoubanLink() {
        return doubanLink;
    }

    public void setDoubanLink(String doubanLink) {
        this.doubanLink = doubanLink;
    }

    public String getFromUrl() {
        return fromUrl;
    }

    public void setFromUrl(String fromUrl) {
        this.fromUrl = fromUrl;
    }

    public LocalDateTime getCreateTime() {
        return createTime;
    }

    public void setCreateTime(LocalDateTime createTime) {
        this.createTime = createTime;
    }

    @Override
    public String toString() {
        return "Torrent{" +
            "torrentId = " + torrentId +
            ", uuid = " + uuid +
            ", userId = " + userId +
            ", type = " + type +
            ", title = " + title +
            ", subTitle = " + subTitle +
            ", uploader = " + uploader +
            ", descr = " + descr +
            ", videoEncode = " + videoEncode +
            ", audioEncode = " + audioEncode +
            ", region = " + region +
            ", edition = " + edition +
            ", team = " + team +
            ", medium = " + medium +
            ", tagList = " + tagList +
            ", coverLink = " + coverLink +
            ", thumbLink = " + thumbLink +
            ", doubanLink = " + doubanLink +
            ", fromUrl = " + fromUrl +
            ", createTime = " + createTime +
        "}";
    }
}

package net.posase.ptools.common.tools;

import com.abercap.mediainfo.api.MediaInfo;
import net.coobird.thumbnailator.Thumbnailator;
import net.coobird.thumbnailator.Thumbnails;
import net.coobird.thumbnailator.util.ThumbnailatorUtils;
import net.posase.ptools.common.downloader.Downloader;
import net.posase.ptools.common.downloader.MvCallBack;
import net.posase.ptools.common.enums.DealType;
import net.posase.ptools.common.enums.ErrorCode;
import net.posase.ptools.common.enums.IState;
import net.posase.ptools.common.exception.ApiException;
import net.posase.ptools.modules.tms.entity.Mv;
import net.posase.ptools.modules.tms.mapper.MvMapper;
import net.posase.ptools.modules.tms.service.MvService;
import org.springframework.beans.factory.annotation.Autowired;

import java.io.File;
import java.io.IOException;
import java.net.MalformedURLException;
import java.net.URL;
import java.util.List;
import java.util.Map;

public class Uploader {

    public static void deal_upload(Mv mv, DealType dealType) {
        if(dealType == DealType.QMusic) {
            // QMusic
            deal_qmv(mv);
        }
    }

    private static void deal_qmv(MvMapper baseMapper, Mv mv, String singer, String song, String version) throws IOException {

        MediaInfo info = new MediaInfo();
        File file = new File(mv.getPath());
        Utils.iAssert(file.exists() && file.isFile(), ErrorCode.MV_VALID);
        info.open(file.getAbsoluteFile());

        // 0. 根据信息链接获取信息

        // 1. 重命名
        // 艺术家 - 歌名.年份.{版本.}分辨率.WEB-DL.视频编码.音频编码-LeagueMV
        Map<String, String> qinfo = parse_qmv_info(mv.getInfoLink());
        if(version != null && version != "")
            version += ".";
        else
            version = "";
        String video_format = info.get(MediaInfo.StreamKind.Video, 0, "Format/String");
        String audio_format = info.get(MediaInfo.StreamKind.Audio, 0, "Format/String");
        int width = Integer.parseInt(info.get(MediaInfo.StreamKind.Video, 0, "Width"));

        String fileName = String.format("%s - %s.%s.%s%s.WEB-DL.%s.%s-LeagueMV",
                singer, song, qinfo.get("year"), version, parse_resolution(width), parse_video_format(video_format), audio_format);
        String[] splits = file.getName().split("\.");
        String suffix = splits[splits.length - 1];
        File new_file = new File(file.getParent() + fileName + ".", suffix);

        if(new_file.exists())
            new_file.delete();
        info.open(new_file);

        mv.setPath(new_file.getAbsolutePath());
        mv.setState(IState.RENAME);
        baseMapper.updateById(mv);

        // 2. 封面
        File cover = new File(new_file.getParent() + fileName + "_cover.jpg");
        if(cover.exists()) {
            Thumbnails.of(new URL(qinfo.get("cover_link"))).size(1280, 720).toFile(cover);
        }

        // 3. 缩略图

        // General 信息
        String fileSize = info.get(MediaInfo.StreamKind.General, 0, "FileSize/String4");
        String duration = info.get(MediaInfo.StreamKind.General, 0, "Duration/String3");
        String bitRate = info.get(MediaInfo.StreamKind.General, 0, "OverallBitRate/String");

        // Video 信息
        String height = info.get(MediaInfo.StreamKind.Video, 0, "Height");
        String frame = info.get(MediaInfo.StreamKind.Video, 0, "FrameRate/String");

        // Audio 信息
        for(int i = 0; i < info.streamCount(MediaInfo.StreamKind.Audio); ++i) {
            bitRate = info.get(MediaInfo.StreamKind.Audio, i, "BitRate/String");
            String format = info.get(MediaInfo.StreamKind.Audio, i, "Format/String");
            String bitRateMode = info.get(MediaInfo.StreamKind.Audio, i, "BitRate_Mode");
            String Channels = info.get(MediaInfo.StreamKind.Audio, i, "Channel(s)/String");
        }
    }

    private static String parse_resolution(int width) {
        if(width > 7500)
            return "4320p";
        else if(width > 3600)
            return "2160p";
        else if(width > 1700)
            return "1080p";
        else
            throw new ApiException(ErrorCode.RESOLUTION);
    }

    private static String parse_video_format(String format) {
        if(format.equals("AVC") || format.startsWith("MPEG-4"))
            return "H264";
        else if(format.equals("HEVC"))
            return "H265";
        return format;
    }

    private static void print_mediainfo(String path) {
        MediaInfo info = new MediaInfo();
        info.open(new File(path));
        Map<MediaInfo.StreamKind, List<Map<String, String>>> map = info.snapshot();
        map.forEach((format, list) -> {
            System.out.println(format);
            System.out.println("------------------------------------------------");
            for(Map<String, String> item : list) {
                item.forEach((k ,v) -> System.out.println(k + ": " + v));
            }
            System.out.println("------------------------------------------------");
        });
    }

    public static void main(String[] args) {
        String path = "C:/Users/Jeson/Desktop/qmmv_0b6bniaa6aaaemahhhmd5vqvi2qab5vaad2a.f0.mp4";
        print_mediainfo(path);
    }
}

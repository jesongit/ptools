package net.posase.ptools.modules.tms.service.impl;

import net.posase.ptools.modules.tms.entity.Torrent;
import net.posase.ptools.modules.tms.mapper.TorrentMapper;
import net.posase.ptools.modules.tms.service.TorrentService;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import org.springframework.stereotype.Service;

/**
 * <p>
 * 种子信息表 服务实现类
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@Service
public class TorrentServiceImpl extends ServiceImpl<TorrentMapper, Torrent> implements TorrentService {

}

package net.posase.ptools.modules.tms.controller;

import cn.dev33.satoken.util.SaResult;
import net.posase.ptools.modules.tms.entity.Mv;
import net.posase.ptools.modules.tms.service.MvService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.validation.annotation.Validated;
import org.springframework.web.bind.annotation.*;

import java.util.List;

/**
 * <p>
 * MV信息表 前端控制器
 * </p>
 *
 * @author posase
 * @since 2023-02-10
 */
@RestController
@RequestMapping("/tms/mv")
public class MvController {

    @Autowired
    MvService mvService;

    @RequestMapping(value = "list", method = RequestMethod.GET)
    public SaResult list() {
        List<Mv> list = mvService.list();;
        return SaResult.data(list);
    }

    @RequestMapping(value = "insert", method = RequestMethod.POST)
    public SaResult insert(@Validated @RequestBody Mv mv) {
        mvService.save(mv);
        return SaResult.data(mv);
    }

    @RequestMapping(value = "delete", method = RequestMethod.DELETE)
    public SaResult delete(@RequestParam("uuid") String uuid) {
        mvService.removeById(uuid);
        return SaResult.ok();
    }

//    @RequestMapping(value = "update", method = RequestMethod.PUT)
//    public SaResult update(@Validated @RequestBody Mv mv_template) {
//        mvService.updateById(mv_template);
//        return SaResult.ok();
//    }

    @RequestMapping(value = "download", method = RequestMethod.GET)
    public SaResult download(@RequestParam("uuid_list") List<String> uuid_list,
                             @RequestParam("path") String path) {
        mvService.download(uuid_list, path);
        return SaResult.ok();
    }

    @RequestMapping(value = "deal", method = RequestMethod.POST)
    public SaResult deal(@Validated @RequestBody Mv mv) {
        mvService.deal(mv);
        return SaResult.ok();
    }
}

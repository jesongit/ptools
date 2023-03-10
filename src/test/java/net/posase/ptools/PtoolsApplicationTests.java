package net.posase.ptools;

import cn.dev33.satoken.SaManager;
import cn.dev33.satoken.secure.SaSecureUtil;
import com.baomidou.mybatisplus.annotation.FieldFill;
import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.generator.FastAutoGenerator;
import com.baomidou.mybatisplus.generator.IFill;
import com.baomidou.mybatisplus.generator.config.OutputFile;
import com.baomidou.mybatisplus.generator.config.po.LikeTable;
import com.baomidou.mybatisplus.generator.config.po.TableField;
import com.baomidou.mybatisplus.generator.config.rules.DateType;
import com.baomidou.mybatisplus.generator.engine.FreemarkerTemplateEngine;
import com.baomidou.mybatisplus.generator.fill.Property;
import net.posase.ptools.common.tools.Utils;
import org.junit.jupiter.api.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.core.env.Environment;

import javax.swing.text.ParagraphView;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

@SpringBootTest
class PtoolsApplicationTests {

    Logger logger = LoggerFactory.getLogger(getClass());

    @Autowired
    private Environment environment;

    @Test
    void test() {
        String key = "LD4pMdPI9R0ruBK";
        String pwd = SaSecureUtil.aesEncrypt(key, "asfdafsdfasfafda");
        System.out.println(pwd);
        System.out.println(pwd.length());
    }

    @Test
    void genRsaKey() throws Exception {
        System.out.println(SaSecureUtil.rsaGenerateKeyPair());
    }

//    @Autowired
//    AdminMapper adminMapper;
//
//    @Test
//    void test() {
//        List<Admin> userList = adminMapper.selectList(null);
//        userList.forEach(System.out::println);
//    }

    @Test
    void generator() {
        String model = "tms";
        String path = System.getProperty("user.dir");
        String url = environment.getProperty("spring.datasource.url");
        String username = environment.getProperty("spring.datasource.username");
        String password = environment.getProperty("spring.datasource.password");

        List<IFill> tabFillList = new ArrayList<>();
        tabFillList.add(new Property("createTime", FieldFill.INSERT));
        tabFillList.add(new Property("updateTime", FieldFill.INSERT_UPDATE));

        FastAutoGenerator.create(url, username, password)
                .globalConfig(builder -> {
                    builder.author("posase")    // ????????????
                            .disableOpenDir()   // ????????????????????????
                            .enableSpringdoc()  // SpringDoc
                            .outputDir(path + "\\src\\main\\java"); // ??????????????????
                })
                .packageConfig(builder -> {
                    builder.parent("net.posase.ptools.modules") // ???????????????
                            .moduleName(model) // ?????????????????????
                            .pathInfo(Collections.singletonMap(OutputFile.xml,
                                    path + "\\src\\main\\resources\\mapper\\" + model)); // ??????mapperXml????????????
                })
//                .injectionConfig(builder -> {
//                })
                .strategyConfig(builder -> {
                    builder.likeTable(new LikeTable(model + "_"))
//                    builder.addInclude("t_simple")    // ???????????????????????????
                            .addTablePrefix(model + "_")  // ?????????????????????
                            .addTableSuffix("_info")
                            .entityBuilder()            // entry ??????
                            .enableLombok()             // Lombok ?????? ???????????? Getter ??? Setter
//                            .addTableFills(new Column("gmt_create", FieldFill.INSERT)) //??????????????????????????????
//                            .addTableFills(new Property("createTime", FieldFill.INSERT))//???????????????????????????????????????
                            .addTableFills(tabFillList)
                            .addTableFills()
//                            .enableTableFieldAnnotation() // ????????????????????????
                            .enableFileOverride()       // ????????????
                            .mapperBuilder()            // mapper ??????
                            .enableFileOverride()
                            .serviceBuilder()           // service ??????
                            .formatServiceFileName("%sService")
                            .enableFileOverride()
                            .controllerBuilder()        // controller ??????
                            .enableRestStyle();         // ??????@RestController
                })
                .templateEngine(new FreemarkerTemplateEngine()) // ??????Freemarker???????????????????????????Velocity????????????
                .execute();
    }
}

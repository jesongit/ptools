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
                    builder.author("posase")    // 设置作者
                            .disableOpenDir()   // 禁止打开输出目录
                            .enableSpringdoc()  // SpringDoc
                            .outputDir(path + "\\src\\main\\java"); // 指定输出目录
                })
                .packageConfig(builder -> {
                    builder.parent("net.posase.ptools.modules") // 设置父包名
                            .moduleName(model) // 设置父包模块名
                            .pathInfo(Collections.singletonMap(OutputFile.xml,
                                    path + "\\src\\main\\resources\\mapper\\" + model)); // 设置mapperXml生成路径
                })
//                .injectionConfig(builder -> {
//                })
                .strategyConfig(builder -> {
                    builder.likeTable(new LikeTable(model + "_"))
//                    builder.addInclude("t_simple")    // 设置需要生成的表名
                            .addTablePrefix(model + "_")  // 设置过滤表前缀
                            .addTableSuffix("_info")
                            .entityBuilder()            // entry 策略
                            .enableLombok()             // Lombok 模式 只会生成 Getter 和 Setter
//                            .addTableFills(new Column("gmt_create", FieldFill.INSERT)) //根据数据库字段名适配
//                            .addTableFills(new Property("createTime", FieldFill.INSERT))//根据生成实体类的属性名适配
                            .addTableFills(tabFillList)
                            .addTableFills()
//                            .enableTableFieldAnnotation() // 数据库自动名注解
                            .enableFileOverride()       // 文件覆盖
                            .mapperBuilder()            // mapper 策略
                            .enableFileOverride()
                            .serviceBuilder()           // service 策略
                            .formatServiceFileName("%sService")
                            .enableFileOverride()
                            .controllerBuilder()        // controller 策略
                            .enableRestStyle();         // 生成@RestController
                })
                .templateEngine(new FreemarkerTemplateEngine()) // 使用Freemarker引擎模板，默认的是Velocity引擎模板
                .execute();
    }
}

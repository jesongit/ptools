package net.posase.ptools;

import com.baomidou.mybatisplus.generator.FastAutoGenerator;
import com.baomidou.mybatisplus.generator.config.OutputFile;
import com.baomidou.mybatisplus.generator.config.po.LikeTable;
import com.baomidou.mybatisplus.generator.engine.FreemarkerTemplateEngine;
import org.junit.jupiter.api.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.core.env.Environment;

import java.util.Collections;
import java.util.List;

@SpringBootTest
class PtoolsApplicationTests {

    Logger logger = LoggerFactory.getLogger(getClass());

    @Autowired
    private Environment environment;

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
        String model = "ums";
        String path = System.getProperty("user.dir");
        String url = environment.getProperty("spring.datasource.url");
        String username = environment.getProperty("spring.datasource.username");
        String password = environment.getProperty("spring.datasource.password");

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
                            .entityBuilder()            // entry 策略
//                            .enableLombok()             // Lombok 模式 只会生成 Getter 和 Setter
                            .enableFileOverride()       // 文件覆盖
                            .mapperBuilder()            // mapper 策略
                            .enableFileOverride()       // 文件覆盖
                            .serviceBuilder()           // service 策略
                            .enableFileOverride()       // 文件覆盖
                            .controllerBuilder()        // controller 策略
                            .enableFileOverride()       // 文件覆盖
                            .enableRestStyle();         // 生成@RestController
                })
                .templateEngine(new FreemarkerTemplateEngine()) // 使用Freemarker引擎模板，默认的是Velocity引擎模板
                .execute();
    }
}

package net.posase.ptools;

import org.mybatis.spring.annotation.MapperScan;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

@SpringBootApplication
@MapperScan("net.posase.ptools.modules.*.mapper")
public class PtoolsApplication {

    public static void main(String[] args) {
        SpringApplication.run(PtoolsApplication.class, args);
    }

}

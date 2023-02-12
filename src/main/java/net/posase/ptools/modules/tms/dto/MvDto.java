package net.posase.ptools.modules.tms.dto;

import jakarta.validation.constraints.NotEmpty;

public class MvDto {

    @NotEmpty
    private String name;

    @NotEmpty
    private String singer;
}

let selected_dir = "";
let active_file = "";
let current_editor = null;

function open(name) {
    if(name.endsWith("/")) {
        select_path(selected_dir+name);
    } else {
        open_file(name);
    }
    reload_path();
}

function reload_path() {
    let path = document.getElementById("path");
    path.replaceChildren();
    let combined = "";
    let list = (selected_dir + active_file).split("/");
    list.reverse();
    list.push("%ROOT%");
    list.reverse();
    for (let bit of list) {
        if(bit.length > 0) {
            combined += bit + "/";
            let v = combined;
            let selector = document.createElement("p");
            selector.textContent = " " + bit + " ";
            path.appendChild(selector);
            if(bit !== active_file) {
                selector.style.cursor = "pointer";
                selector.onmouseenter = async function () {
                    selector.style.textDecoration = "underline";
                };
                selector.onmouseleave = async function () {
                    selector.style.textDecoration = "";
                };
                selector.onclick = async function () {
                    select_path(v);
                    reload_path();
                };
                let slash = document.createElement("p");
                slash.textContent = "/";
                path.appendChild(slash);
            }
        }
    }
    const url = new URL(window.location.href);
    url.searchParams.set("active_file", active_file);
    url.searchParams.set("selected_dir", selected_dir);
    window.history.replaceState({}, "", url);
}

let open_file_hash = "";

function open_file(to) {
    active_file = to;
    document.getElementById("file-area").replaceChildren();
    document.getElementById("file-area").style.display = "block";
    document.getElementById("path-select").style.display = "none";

    const request = new XMLHttpRequest();
    request.open("POST", "api/get_file");
    request.onload = async function () {
        if (request.status === 200) {
            let val = JSON.parse(request.responseText);
            if (current_editor != null) {
                current_editor.delete();
                current_editor = null;
            }
            const module = await import("./editors/"+val.editor+".js");
            current_editor = module.default;

            open_file_hash = val.hash;
            if(current_editor.can_save()) {
                let save_button = createElement(`<svgfrom src="save.svg"></svgfrom>`);
                save_button.classList.add("update_button");
                save_button.style.background = "#04AA6D";
                save_button.style.outlineColor = "#04AA6D";
                save_button.onclick = save;
                save_button.style.position = "fixed";
                save_button.style.right = "10px";
                document.getElementById("file-area").appendChild(save_button);
            }
            current_editor.on_opened(val);
        }
    }
    request.send("."+(selected_dir + active_file));
}

function save(event) {
    save_internal(event, false);
}

function save_internal(event, bypass) {
    const request = new XMLHttpRequest();
    request.open("POST", "api/save_file");
    request.setRequestHeader("Content-Type", "application/json");
    request.onload = async function () {
        if (request.status === 200) {
            current_editor.on_save();
        } else if(request.status === 422 && request.responseText === "Wrong hash") {
            if(confirm("The file has been changed since you started editing. Save anyway?")) {
                save_internal(event, true);
            }
        }
    }
    let data = {
        old_hash: open_file_hash,
        bypass_hash: bypass,
        path: "."+(selected_dir + active_file),
        content: current_editor.get_save_content()
    }
    request.send(JSON.stringify(data));
}

function select_path(to) {
    if(active_file.length !== 0 && current_editor.can_save() && current_editor.has_changed()) {
        if(!confirm("Do you really want to close this file - it has not been saved!")) {
            return;
        }
    }
    //if you were in a file and left go back to where it was
    let last_file_name = "NONE";
    if(active_file.length !== 0) {
        last_file_name = active_file;
    }
    active_file = "";
    selected_dir = to.replace("%ROOT%", "/").replace("//", "/");
    document.getElementById("file-area").style.display = "none";
    document.getElementById("path-select").style.display = "block";
    document.getElementById("path-select").replaceChildren();
    const request = new XMLHttpRequest();
    request.open("POST", "api/list_dir");
    request.onload = async function () {
        if (request.status === 200) {
            if (selected_dir.startsWith("/logs/")) {
                let warning = document.createElement("a");
                warning.href = "/logs";
                warning.textContent = "See the logs page for a better viewing experience";
                document.getElementById("path-select").appendChild(warning);
            }
            if (selected_dir.startsWith("/mods/")) {
                let warning = document.createElement("a");
                warning.href = "/mods";
                warning.textContent = "See the mods page for a better viewing experience";
                document.getElementById("path-select").appendChild(warning);
            }
            if (request.responseText.length === 0) {
                let info = document.createElement("p");
                info.textContent = "Directory is empty.";
                document.getElementById("path-select").appendChild(info);
            } else {
                let last_divider = null;
                for (let path of request.responseText.split("\n")) {
                    if (path.length === 0) continue;
                    let selector = document.createElement("p");
                    selector.textContent = path;
                    selector.style.cursor = "pointer";
                    selector.onmouseenter = async function () {
                        selector.style.textDecoration = "underline";
                    };
                    selector.onmouseleave = async function () {
                        selector.style.textDecoration = "";
                    };
                    selector.onclick = async function () {
                        open(path);
                    };
                    document.getElementById("path-select").appendChild(selector);

                    let icon = null;
                    if (selected_dir.includes("/data/maps/") && path.endsWith(".dat")) {
                        //svgs that already have their data dont trigger onload
                        icon = createElement(`<img src="icons/file-signal.svg" alt="">`);
                        icon.style.paddingRight = "0.2em";
                        icon.style.height = "1em";
                        icon.style.width = "auto";
                        icon.loading = "lazy";
                        icon.onload = () => {
                            icon = replaceElement(icon, `<svgfrom src="file-signal.svg"></svgfrom>`);
                            icon.style.paddingRight = "0.2em";
                            create_map("." + selected_dir + path, (img) => {
                                icon.remove();
                                img.style.paddingRight = "0.2em";
                                img.style.height = "1em";
                                img.style.width = "auto";
                                selector.prepend(img);
                            });
                        };
                    } else if (path.endsWith("/")) {
                        icon = createElement(`<svgfrom src="folder.svg"></svgfrom>`);
                    } else {
                        if (path.endsWith(".txt") || path.endsWith(".md") || path.endsWith(".log")) {
                            icon = createElement(`<svgfrom src="file-text.svg"></svgfrom>`);
                        } else if (path.endsWith(".json") || path.endsWith(".toml") || path.endsWith(".yaml") || path.endsWith(".properties") || path.endsWith(".json5")) {
                            icon = createElement(`<svgfrom src="file-braces-corner.svg"></svgfrom>`);
                        } else if (path.endsWith(".bat") || path.endsWith(".sh") || path.endsWith(".lua")) {
                            icon = createElement(`<svgfrom src="file-terminal.svg"></svgfrom>`);
                        } else if (path.endsWith(".zip") || path.endsWith(".jar")) {
                            icon = createElement(`<svgfrom src="file-archive.svg"></svgfrom>`);
                        } else icon = createElement(`<svgfrom src="file.svg"></svgfrom>`);
                    }
                    if (icon != null) {
                        icon.style.paddingRight = "0.2em";
                        selector.prepend(icon);
                    }

                    let divider = document.createElement("div");
                    last_divider = divider;
                    divider.style.border = "2px solid var(--border)";
                    divider.style.height = "0";
                    divider.style.borderRadius = "2px";
                    divider.style.width = "100%";
                    document.getElementById("path-select").appendChild(divider);

                    if(path === last_file_name) {
                        selector.scrollIntoView();
                    }
                }
                if (last_divider != null) last_divider.remove();
            }
        }
    }
    let get = selected_dir;
    if(get.startsWith("/")) {
        get = get.slice(1);
    }
    request.send("./"+get);
}

window.addEventListener("load", () => {
    const params = new URLSearchParams(window.location.search);
    if(params.has("active_file") && params.has("selected_dir")) {
        let f = params.get("active_file");
        let d = params.get("selected_dir");
        if(f.length > 0) {
            selected_dir = d;
            open_file(f);
        } else select_path(d);
        reload_path();
    } else open("/");
});

function create_map(path, on_created) {
    const request = new XMLHttpRequest();
    request.responseType = 'blob';
    request.open("POST", "api/map_data");
    request.onload = async function () {
        const blob = request.response;
        const decompressedStream = blob.stream().pipeThrough(new DecompressionStream('gzip'));
        const decompressedBlob = await new Response(decompressedStream).blob();

        const bytes = await decompressedBlob.bytes();

        let base_colours = [
            [0, 0, 0, 0], // 0 NONE
            [127, 178, 56, 255], // 1 GRASS
            [247, 233, 163, 255], // 2 SAND
            [199, 199, 199, 255], // 3 WOOL
            [255, 0, 0, 255], // 4 FIRE
            [160, 160, 255, 255], // 5 ICE
            [167, 167, 167, 255], // 6 METAL
            [0, 124, 0, 255], // 7 PLANT
            [255, 255, 255, 255], // 8 SNOW
            [164, 168, 184, 255], // 9 CLAY
            [151, 109, 77, 255], // 10 DIRT
            [112, 112, 112, 255], // 11 STONE
            [64, 64, 255, 255], // 12 WATER
            [143, 119, 72, 255], // 13 WOOD
            [255, 252, 245, 255], // 14 QUARTZ
            [216, 127, 51, 255], // 15 COLOR_ORANGE
            [178, 76, 216, 255], // 16 COLOR_MAGENTA
            [102, 153, 216, 255], // 17 COLOR_LIGHT_BLUE
            [229, 229, 51, 255], // 18 COLOR_YELLOW
            [127, 204, 25, 255], // 19 COLOR_LIGHT_GREEN
            [242, 127, 165, 255], // 20 COLOR_PINK
            [76, 76, 76, 255], // 21 COLOR_GRAY
            [153, 153, 153, 255], // 22 COLOR_LIGHT_GRAY
            [76, 127, 153, 255], // 23 COLOR_CYAN
            [127, 63, 178, 255], // 24 COLOR_PURPLE
            [51, 76, 178, 255], // 25 COLOR_BLUE
            [102, 76, 51, 255], // 26 COLOR_BROWN
            [102, 127, 51, 255], // 27 COLOR_GREEN
            [153, 51, 51, 255], // 28 COLOR_RED
            [25, 25, 25, 255], // 29 COLOR_BLACK
            [250, 238, 77, 255], // 30 GOLD
            [92, 219, 213, 255], // 31 DIAMOND
            [74, 128, 255, 255], // 32 LAPIS
            [0, 217, 58, 255], // 33 EMERALD
            [129, 86, 49, 255], // 34 PODZOL
            [112, 2, 0, 255], // 35 NETHER
            [209, 177, 161, 255], // 36 TERRACOTTA_WHITE
            [159, 82, 36, 255], // 37 TERRACOTTA_ORANGE
            [149, 87, 108, 255], // 38 TERRACOTTA_MAGENTA
            [112, 108, 138, 255], // 39 TERRACOTTA_LIGHT_BLUE
            [186, 133, 36, 255], // 40 TERRACOTTA_YELLOW
            [103, 117, 53, 255], // 41 TERRACOTTA_LIGHT_GREEN
            [160, 77, 78, 255], // 42 TERRACOTTA_PINK
            [57, 41, 35, 255], // 43 TERRACOTTA_GRAY
            [135, 107, 98, 255], // 44 TERRACOTTA_LIGHT_GRAY
            [87, 92, 92, 255], // 45 TERRACOTTA_CYAN
            [122, 73, 88, 255], // 46 TERRACOTTA_PURPLE
            [76, 62, 92, 255], // 47 TERRACOTTA_BLUE
            [76, 50, 35, 255], // 48 TERRACOTTA_BROWN
            [76, 82, 42, 255], // 49 TERRACOTTA_GREEN
            [142, 60, 46, 255], // 50 TERRACOTTA_RED
            [37, 22, 16, 255], // 51 TERRACOTTA_BLACK
            [189, 48, 49, 255], // 52 CRIMSON_NYLIUM
            [148, 63, 97, 255], // 53 CRIMSON_STEM
            [92, 25, 29, 255], // 54 CRIMSON_HYPHAE
            [22, 126, 134, 255], // 55 WARPED_NYLIUM
            [58, 142, 140, 255], // 56 WARPED_STEM
            [86, 44, 62, 255], // 57 WARPED_HYPHAE
            [20, 180, 133, 255], // 58 WARPED_WART_BLOCK
            [100, 100, 100, 255], // 59 DEEPSLATE
            [216, 175, 147, 255], // 60 RAW_IRON
            [127, 167, 150, 255], // 61 GLOW_LICHEN
        ];

        let colours = [];

        for (let base_colour of base_colours) {
            colours.push([base_colour[0] * 0.71, base_colour[1] * 0.71, base_colour[2] * 0.71, base_colour[3]]);
            colours.push([base_colour[0] * 0.86, base_colour[1] * 0.86, base_colour[2] * 0.86, base_colour[3]]);
            colours.push(base_colour);
            colours.push([base_colour[0] * 0.53, base_colour[1] * 0.53, base_colour[2] * 0.53, base_colour[3]]);
        }

        let size = 128;

        const canvas = document.createElement('canvas');
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext('2d');

        const imgData = ctx.createImageData(size, size);
        const data = imgData.data;

        let imgIndex = 0;

        for (let i = 0; i < bytes.length; i++) {
            let colour = colours[bytes[i]];
            data[imgIndex] = colour[0];
            data[imgIndex + 1] = colour[1];
            data[imgIndex + 2] = colour[2];
            data[imgIndex + 3] = colour[3];
            imgIndex += 4;
        }

        ctx.putImageData(imgData, 0, 0);

        const dataUrl = canvas.toDataURL();
        const img = document.createElement('img');
        img.src = dataUrl;
        img.style.imageRendering = "pixelated";
        on_created(img);
    };
    request.send(path);
}
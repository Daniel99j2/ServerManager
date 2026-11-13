export default {
    can_save() {
        return false;
    },

    has_changed() {
        return false;
    },

    on_save() {
    },

    on_opened(val) {
        create_map("." + selected_dir + active_file, (img) => {
            img.style.height = "min(60vw, 60vh)";
            document.getElementById("file-area").appendChild(img);
        }, () => {
            let v = document.createElement("h3");
            v.textContent = "Map could not be loaded"
            document.getElementById("file-area").appendChild(v);
        });
    },

    get_save_content() {
        return "";
    },

    delete() {
    }
}
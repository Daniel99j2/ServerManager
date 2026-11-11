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
        });
    },

    get_save_content() {
        return "";
    },

    delete() {
    }
}
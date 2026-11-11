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
        let v = document.createElement("h3");
        v.textContent = "Binary file"
        document.getElementById("file-area").appendChild(v);
    },

    get_save_content() {
        return "";
    },

    delete() {
        
    }
}
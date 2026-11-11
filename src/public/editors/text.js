export default {
    changed: false,
    editor: null,
    latest_content: "",
    message_handler: null,

    can_save() {
        return true;
    },

    has_changed() {
        return this.changed;
    },

    on_save() {
        this.changed = false;
    },

    on_opened(val) {
        this.changed = false;
        this.latest_content = val.content;

        let lang = "plaintext";
        if (active_file.endsWith(".ini")) lang = "ini";
        else if (active_file.endsWith(".md")) lang = "markdown";
        else if (active_file.endsWith(".xml")) lang = "xml";
        else if (active_file.endsWith(".yaml")) lang = "yaml";
        else if (active_file.endsWith(".json") || active_file.endsWith(".json5")) lang = "json";
        else if (active_file.endsWith(".lua")) lang = "lua";
        else if (active_file.endsWith(".bat")) lang = "bat";
        else if (active_file.endsWith(".sh")) lang = "shell";
        const url = new URL('https://embeddable-monaco.lukasbach.com');
        const params = new URLSearchParams({
            lang: lang,
            theme: "vs-dark",
            code: "Loading..."
        });
        url.search = params.toString();

        this.editor = createElement("<iframe src=\"URL\" id=\"iframe\" style=\"border: 0; width: 100vw; height: auto; margin-top: 30px;\"></iframe>".replace("URL", url.toString()));
        document.getElementById("file-area").appendChild(this.editor);
        this.editor.addEventListener('load', () => {
            this.editor.contentWindow.postMessage({type: "change-value", value: val.content}, "*");
        });

        //apparently its 'window' not 'editor'
        this.message_handler = (e) => {
            if (e.data.type === "change") {
                if (this.latest_content !== e.data.value) {
                    this.latest_content = e.data.value;
                    this.changed = true;
                }
            }
        }
        window.addEventListener('message', this.message_handler);
    },

    get_save_content() {
        return this.latest_content;
    },

    delete() {
        window.removeEventListener("message", this.message_handler);
    }
}
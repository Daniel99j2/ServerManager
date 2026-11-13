request("GET", "api/all_logs", async (out) => {
    if (out.success) {
        document.getElementById("file_name").replaceChildren();
        out.as_text().split("\n").sort().reverse().forEach((name) => {
            console.log(name)
            let element = document.createElement('option');
            element.value = name;
            element.textContent = name;
            document.getElementById("file_name").appendChild(element);
        });
    }
});

document.getElementById("file_name").addEventListener('change', (event) => {
    console.log('Selected value:', event.target.value);
    document.getElementById("embed").src = "console_embed.html?name="+event.target.value;
});

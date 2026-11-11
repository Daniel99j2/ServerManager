use std::fs;
//use build_print::println;

fn main() {
    println!("cargo::rerun-if-changed=src/public/");
    read("src/public");
}

fn read(x: &str) {
    for i in fs::read_dir(x).expect("Couldnt read site files") {
        if i.is_ok() {
            let unwrapped = i.unwrap();
            let file_type = unwrapped.file_type().unwrap();
            if file_type.is_file() {
                let path = unwrapped.path().into_string().unwrap().to_string().replace("\\", "/");
                dbg!(unwrapped.path());
                let mut contents = fs::read(unwrapped.path()).unwrap();
                if path.ends_with(".html") || path.ends_with(".js") {
                    let mut c = str::from_utf8(&contents).unwrap_or("Error parsing").to_string();
                    if c.contains("<body customproperties") {
                        let (_, remainder) = c.split_once("<body customproperties").expect("How did we get here?");
                        match remainder.split_once(">") {
                            None => {}
                            Some((target, _)) => {
                                if target.contains("withsidebar") {
                                    c = c.replace(format!("<body customproperties{}>", target).as_str(), format!("{}{}", "<body>", fs::read_to_string("src/public/sidebar.html").expect("Couldnt get sidebar").replace("{}", match target.contains("topalign") {
                                        true => {
                                            "justify-content: center;"
                                        }
                                        false => {
                                            ""
                                        }
                                    })).as_str()).replace("</body>", "</div></body>");
                                }
                            }
                        };
                    }
                    while c.contains("<svgfrom src=\"") {
                        let copied_c = c.clone();
                        let (_, remainder) = copied_c.split_once("<svgfrom src=\"").expect("How did we get here?");
                        match remainder.split_once("\"") {
                            None => {
                                break;
                            }
                            Some((target, _)) => {
                                let name = target;
                                let svg = fs::read_to_string(format!("src/public/icons/{}", name));
                                if svg.is_ok() {
                                    let svg = svg.unwrap().replace("<svg", "style=\"height:1em; width:auto; vertical-align:-0.125em\"");
                                    //its not pretty but it works!
                                    c = c
                                        .replacen(
                                            format!("<svgfrom src=\"{}>", remainder.split_once(">").unwrap().0).as_str(),
                                            format!("<svg {} {}", remainder.split_once(">").unwrap().0.replace(format!("{}\"", name).as_str(), ""), svg).as_str(), 1)
                                        .replacen("</svgfrom>", "", 1);
                                } else {
                                    c = format!("Unknown SVG {}", name);
                                }
                            }
                        };
                        let _ = copied_c; //so it isnt dropped
                    }
                    contents = c.into_bytes();
                }

                fs::write(format!("src/gen/{}", path.replace("src/public/", "")), contents).unwrap();
            } else if file_type.is_dir() {
                read(unwrapped.path().to_str().unwrap());
            }
        }
    };

}
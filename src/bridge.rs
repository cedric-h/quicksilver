use std::os::raw::c_char;

extern "C" {
    pub fn create_context(width: u32, height: u32);
    pub fn load_image(string: *const c_char) -> bool;
    pub fn get_image_id() -> u32;
    pub fn get_image_width() -> i32;
    pub fn get_image_height() -> i32;
    pub fn log_num(x: f32);
    pub fn load_vertex_shader(shader: u32);
    pub fn load_frag_shader(shader: u32);
}

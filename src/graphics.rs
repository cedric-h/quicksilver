//! A module to draw 2D graphics in a window
//!  It also includes image loading



mod color;
mod image;
mod mesh;
mod projection;
mod vertex;

pub use self::color::Color;
pub use self::image::Image;
pub use self::mesh::Mesh;
pub use self::projection::orthographic;
pub use self::vertex::{DrawGroup, Vertex};

use crate::geom::*;
use golem::{*, buffer::*, program::*, objects::*};
use mint::*;

// TODO: should projection be handled GPU-side?
// TODO: image views

pub struct Graphics {
    ctx: Context,
    vb: VertexBuffer,
    eb: ElementBuffer,
    shader: ShaderProgram,
    vertex_data: Vec<f32>,
    index_data: Vec<u32>,
    uniforms: Vec<(usize, Option<Image>, Option<ColumnMatrix3<f32>>)>,
}

impl Graphics {
    pub(crate) fn new(ctx: Context) -> Graphics {
        let mut shader = ctx.new_shader(ShaderDescription {
            vertex_input: &[
                Attribute::Vector(4, "vert_color"),
                Attribute::Vector(2, "vert_position"),
                Attribute::Vector(2, "vert_uv"),
            ],
            fragment_input: &[
                Attribute::Vector(4, "frag_color"),
                Attribute::Vector(2, "frag_uv"),
            ],
            uniforms: &[
                Uniform::new("image", UniformType::Sampler(2)),
                Uniform::new("projection", UniformType::Matrix(3)),
            ],
            vertex_shader: r#" void main() {
                vec3 transformed = projection * vec3(vert_position, 1.0);
                gl_Position = vec4(transformed.xy, 0, 1);
                frag_uv = vert_uv;
                frag_color = vert_color;
            }"#,
            fragment_shader:
            r#" void main() {
                vec4 tex = vec4(1);
                if(frag_uv.x < 0.0 && frag_uv.y < 0.0) {
                    tex = texture(image, frag_uv);
                }
                gl_FragColor = tex * frag_color;
            }"#
        }).unwrap();
        let vb = ctx.new_vertex_buffer().unwrap();
        let eb = ctx.new_element_buffer().unwrap();
        shader.bind(&vb);

        Graphics {
            ctx,
            shader,
            vb,
            eb,
            vertex_data: Vec::new(),
            index_data: Vec::new(),
            uniforms: Vec::new(),
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.ctx.set_clear_color(color.r, color.g, color.b, color.a);
        self.ctx.clear();
    }

    pub fn set_projection(&mut self, transform: ColumnMatrix3<f32>) {
        let head = self.index_data.len() / 3;
        self.uniforms.push((head, None, Some(transform)));
    }

    pub fn draw_vertices(&mut self, vertices: impl Iterator<Item = Vertex>, triangles: impl Iterator<Item = [u32; 3]>, image: Option<&Image>) {
        // We need to offset every triangle
        // In the input, the 0th index is the 0th provided vertex
        // In the GL buffer, the 0th index will be the first vertex we ever inserted
        // The number of vertices we've inserted is the length over the size of one insertion
        let offset = self.vertex_data.len() / 9;

        for vertex in vertices {
            let uv = vertex.uv.unwrap_or(Vector2 { x: -1.0, y: -1.0 });
            self.vertex_data.extend_from_slice(&[
                vertex.color.r, vertex.color.g, vertex.color.b, vertex.color.a,
                vertex.pos.x, vertex.pos.y,
                uv.x, uv.y,
            ]);
        }

        let tri_offset = offset as u32;
        for triangle in triangles {
            self.index_data.extend(triangle.iter().map(|index| *index + tri_offset));
        }

        let insert_new_image = match (image, self.uniforms.last_mut()) {
            // If this is the first draw, we need to add a uniform to it
            (_, None) => true,
            // Don't bother inserting a new uniform if we're not adding a new image and we already
            // have one, though
            (None, _) => false,
            // If we're inserting an image and there was an old one, check if they match
            (Some(image), Some((_, Some(old_image), _))) => std::rc::Rc::ptr_eq(&image.0, &old_image.0),
            // If we're inserting an image and there wasn't one, we can just over-write the
            // previous range. Therefore we don't need to insert
            (Some(image), Some(old)) => {
                old.1 = Some(image.clone());

                false
            },
        };
        if insert_new_image {
            self.uniforms.push((offset, image.cloned(), None));
        }
    }

    pub fn draw_mesh(&mut self, mesh: &Mesh) {
        self.draw_vertices(
            mesh.vertices.iter().cloned(),
            mesh.group.triangles.iter().cloned(),
            mesh.group.image.as_ref()
        );
    }

    pub fn draw_polygon(&mut self, points: &[Vector2<f32>], color: Color) {
        let vertices = points.iter().cloned().map(|pos| Vertex { pos, uv: None, color });
        let indices = std::iter::repeat(())
            .take(points.len() - 2)
            .enumerate()
            .map(|(idx, _)| idx as u32)
            .map(|idx| [0, idx + 1, idx + 2]);
        self.draw_vertices(vertices, indices, None);
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.draw_polygon(&[
            rect.min,
            Vector2 { x: rect.min.x, y: rect.max.y },
            rect.max,
            Vector2 { x: rect.max.x, y: rect.min.y },
        ], color);
    }

    pub fn flush(&mut self) -> Result<(), GolemError> {
        self.vb.set_data(self.vertex_data.as_slice());
        self.eb.set_data(self.index_data.as_slice());
        self.shader.set_uniform("image", UniformValue::Int(0))?;
        for index in 0..self.uniforms.len() {
            let uniform = &self.uniforms[index];
            let next = self.uniforms.get(index + 1);

            let (start, image, projection) = uniform;
            let end = match next {
                Some((end, _, _)) => *end,
                None => self.index_data.len()
            };
            // If we're switching what image to use, do so now
            if let Some(image) = image {
                image.0.bind(0);
            }
            // If we're switching what projection to use, do so now
            if let Some(projection) = projection {
                let matrix: [f32; 9] = (*projection).into();
                self.shader.set_uniform("projection", UniformValue::Matrix3(matrix))?;
            }

            if *start != end {
                self.ctx.draw(&self.eb, *start..end).unwrap();
            }
        }
        self.vertex_data.clear();
        self.index_data.clear();
        self.uniforms.clear();

        Ok(())
    }


    pub fn present(&mut self, win: &blinds::Window) -> Result<(), GolemError> {
        self.flush()?;
        win.present();

        Ok(())
    }
}
/*
mod animation;
mod atlas;
mod blend_mode;
mod color;
mod drawable;
#[cfg(feature="fonts")] mod font;
#[cfg(feature="lyon")] mod lyon;
mod image;
mod image_scale_strategy;
#[cfg(feature="immi")] mod immi;
mod mesh;
mod resize;
mod surface;
mod vertex;
mod view;

pub use self::{
    animation::Animation,
    atlas::{Atlas, AtlasError, AtlasItem},
    blend_mode::BlendMode,
    color::Color,
    drawable::{Background, Drawable},
    image::{Image, ImageError, PixelFormat},
    image_scale_strategy::ImageScaleStrategy,
    mesh::Mesh,
    resize::ResizeStrategy,
    surface::Surface,
    vertex::{Vertex, GpuTriangle},
    view::View,
};
#[cfg(feature="fonts")] pub use self::font::{Font, FontStyle};
#[cfg(feature="lyon")] pub use self::lyon::ShapeRenderer;
#[cfg(feature = "immi")] pub use self::immi::{create_immi_ctx, ImmiStatus, ImmiRender};
*/

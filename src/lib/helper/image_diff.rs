use image::DynamicImage;

pub fn parse_hex_color(hex: &str) -> anyhow::Result<[u8; 3]> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        anyhow::bail!(
            "Invalid hex color format: '{}'. Expected 6 hex digits (e.g., ff0000 for red)",
            hex
        );
    }

    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| {
        anyhow::anyhow!(
            "Invalid hex color: '{}'. First two digits '{}' are not valid hex",
            hex,
            &hex[0..2]
        )
    })?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| {
        anyhow::anyhow!(
            "Invalid hex color: '{}'. Middle two digits '{}' are not valid hex",
            hex,
            &hex[2..4]
        )
    })?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| {
        anyhow::anyhow!(
            "Invalid hex color: '{}'. Last two digits '{}' are not valid hex",
            hex,
            &hex[4..6]
        )
    })?;

    Ok([r, g, b])
}

pub fn image_diff(
    before: &DynamicImage,
    after: &DynamicImage,
    changed_color: [u8; 3],
) -> anyhow::Result<DynamicImage> {
    use image::{GenericImage, GenericImageView, Rgba};
    use std::cmp::max;
    let (after_width, after_height) = after.dimensions();
    let (before_width, before_height) = before.dimensions();
    let width = max(after_width, before_width);
    let height = max(after_height, before_height);
    let mut result = DynamicImage::new_rgba8(width, height);

    for y in 0..height {
        for x in 0..width {
            let new_color: [u8; 4];
            let pixel: Rgba<u8>;
            if x >= before_width || y >= before_height || x >= after_width || y >= after_height {
                new_color = [changed_color[0], changed_color[1], changed_color[2], 255];
                pixel = Rgba(new_color);
            } else {
                let before_pixel: Rgba<u8> = before.get_pixel(x, y);
                let after_pixel: Rgba<u8> = after.get_pixel(x, y);
                let alpha = before_pixel[3];

                let is_diff = before_pixel[0] != after_pixel[0]
                    || before_pixel[1] != after_pixel[1]
                    || before_pixel[2] != after_pixel[2];

                let mut new_red = after_pixel[0];
                let mut new_green = after_pixel[1];
                let mut new_blue = after_pixel[2];
                if is_diff {
                    new_red = changed_color[0];
                    new_green = changed_color[1];
                    new_blue = changed_color[2];
                }

                new_color = [new_red, new_green, new_blue, alpha];
                pixel = Rgba(new_color);
            }
            result.put_pixel(x, y, pixel);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage};

    fn create_test_image(width: u32, height: u32, color: [u8; 3]) -> DynamicImage {
        let mut img: RgbaImage = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                img.put_pixel(x, y, Rgba([color[0], color[1], color[2], 255]));
            }
        }
        DynamicImage::from(img)
    }

    #[test]
    fn test_parse_hex_color_valid() {
        assert_eq!(parse_hex_color("ff0000").unwrap(), [255, 0, 0]);
        assert_eq!(parse_hex_color("#ff0000").unwrap(), [255, 0, 0]);
        assert_eq!(parse_hex_color("00ff00").unwrap(), [0, 255, 0]);
        assert_eq!(parse_hex_color("0000ff").unwrap(), [0, 0, 255]);
        assert_eq!(parse_hex_color("123456").unwrap(), [0x12, 0x34, 0x56]);
        assert_eq!(parse_hex_color("#abcdef").unwrap(), [0xab, 0xcd, 0xef]);
    }

    #[test]
    fn test_parse_hex_color_invalid_length() {
        assert!(parse_hex_color("ff00").is_err());
        assert!(parse_hex_color("ff000").is_err());
        assert!(parse_hex_color("ff00000").is_err());
        assert!(parse_hex_color("").is_err());
        assert!(parse_hex_color("#ff00000").is_err());
    }

    #[test]
    fn test_parse_hex_color_invalid_hex() {
        assert!(parse_hex_color("gg0000").is_err());
        assert!(parse_hex_color("00gg00").is_err());
        assert!(parse_hex_color("0000gg").is_err());
        assert!(parse_hex_color("#gg0000").is_err());
        assert!(parse_hex_color("ffg000").is_err());
    }

    #[test]
    fn test_parse_hex_color_with_hash() {
        assert_eq!(parse_hex_color("#ff0000").unwrap(), [255, 0, 0]);
        assert_eq!(parse_hex_color("#00ff00").unwrap(), [0, 255, 0]);
        assert_eq!(parse_hex_color("#0000ff").unwrap(), [0, 0, 255]);
    }

    #[test]
    fn test_parse_hex_color_mixed_case() {
        assert_eq!(parse_hex_color("Ff0000").unwrap(), [255, 0, 0]);
        assert_eq!(parse_hex_color("00Ff00").unwrap(), [0, 255, 0]);
        assert_eq!(parse_hex_color("0000Ff").unwrap(), [0, 0, 255]);
        assert_eq!(parse_hex_color("#aBcDeF").unwrap(), [0xab, 0xcd, 0xef]);
    }

    #[test]
    fn test_parse_hex_color_zero_values() {
        assert_eq!(parse_hex_color("000000").unwrap(), [0, 0, 0]);
        assert_eq!(parse_hex_color("#000000").unwrap(), [0, 0, 0]);
    }

    #[test]
    fn test_parse_hex_color_max_values() {
        assert_eq!(parse_hex_color("ffffff").unwrap(), [255, 255, 255]);
        assert_eq!(parse_hex_color("#ffffff").unwrap(), [255, 255, 255]);
    }

    #[test]
    fn test_image_diff_same_images() {
        let color = [100, 150, 200];
        let before = create_test_image(10, 10, color);
        let after = create_test_image(10, 10, color);

        let result = image_diff(&before, &after, [255, 0, 0]).unwrap();

        for y in 0..10 {
            for x in 0..10 {
                let pixel = result.get_pixel(x, y);
                assert_eq!(pixel[0], color[0]);
                assert_eq!(pixel[1], color[1]);
                assert_eq!(pixel[2], color[2]);
            }
        }
    }

    #[test]
    fn test_image_diff_changed_pixels_use_specified_color() {
        let before_color = [100, 150, 200];
        let after_color = [50, 75, 100];
        let diff_color = [0, 255, 0];

        let before = create_test_image(10, 10, before_color);
        let after = create_test_image(10, 10, after_color);

        let result = image_diff(&before, &after, diff_color).unwrap();

        for y in 0..10 {
            for x in 0..10 {
                let pixel = result.get_pixel(x, y);
                assert_eq!(pixel[0], diff_color[0], "Red mismatch at ({}, {})", x, y);
                assert_eq!(pixel[1], diff_color[1], "Green mismatch at ({}, {})", x, y);
                assert_eq!(pixel[2], diff_color[2], "Blue mismatch at ({}, {})", x, y);
                assert_eq!(pixel[3], 255, "Alpha should be 255 at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn test_image_diff_out_of_bounds_use_diff_color() {
        let before = create_test_image(10, 10, [100, 150, 200]);
        let after = create_test_image(15, 15, [50, 75, 100]);
        let diff_color = [0, 0, 255];

        let result = image_diff(&before, &after, diff_color).unwrap();

        for y in 0..10 {
            for x in 0..10 {
                let pixel = result.get_pixel(x, y);
                assert_eq!(pixel[0], diff_color[0]);
                assert_eq!(pixel[1], diff_color[1]);
                assert_eq!(pixel[2], diff_color[2]);
            }
        }

        for y in 10..15 {
            for x in 0..15 {
                let pixel = result.get_pixel(x, y);
                assert_eq!(pixel[0], diff_color[0]);
                assert_eq!(pixel[1], diff_color[1]);
                assert_eq!(pixel[2], diff_color[2]);
                assert_eq!(pixel[3], 255);
            }
        }

        for x in 10..15 {
            for y in 0..10 {
                let pixel = result.get_pixel(x, y);
                assert_eq!(pixel[0], diff_color[0]);
                assert_eq!(pixel[1], diff_color[1]);
                assert_eq!(pixel[2], diff_color[2]);
                assert_eq!(pixel[3], 255);
            }
        }
    }

    #[test]
    fn test_image_diff_partial_overlap() {
        let before = create_test_image(10, 10, [100, 150, 200]);
        let after_img = create_test_image(10, 10, [100, 150, 200])
            .as_rgba8()
            .unwrap()
            .clone();
        let mut after_img = after_img;
        after_img.put_pixel(5, 5, Rgba([200, 50, 100, 255]));
        let after = DynamicImage::from(after_img);
        let diff_color = [255, 255, 0];

        let result = image_diff(&before, &after, diff_color).unwrap();

        for y in 0..10 {
            for x in 0..10 {
                if x == 5 && y == 5 {
                    let pixel = result.get_pixel(x, y);
                    assert_eq!(pixel[0], diff_color[0]);
                    assert_eq!(pixel[1], diff_color[1]);
                    assert_eq!(pixel[2], diff_color[2]);
                } else {
                    let pixel = result.get_pixel(x, y);
                    assert_eq!(pixel[0], 100);
                    assert_eq!(pixel[1], 150);
                    assert_eq!(pixel[2], 200);
                }
            }
        }
    }
}

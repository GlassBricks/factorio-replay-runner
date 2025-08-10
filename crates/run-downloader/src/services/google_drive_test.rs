#[cfg(test)]
mod tests {
    use crate::{FileService, GoogleDriveService};

    #[test]
    fn test_link_detection() {
        let test_cases = vec![
            (
                "https://drive.google.com/file/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms/view",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
            (
                "https://drive.google.com/open?id=1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
            (
                "https://docs.google.com/file/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
        ];

        for (url, expected_id) in test_cases {
            let link = GoogleDriveService::detect_link(url);
            assert_eq!(link, Some(expected_id.to_string()));
        }
    }

    #[test]
    fn test_no_links_detected() {
        let no_link_text = "This is just some text without any Google Drive links";
        let link = GoogleDriveService::detect_link(no_link_text);
        assert!(link.is_none());
    }

    #[test]
    fn test_multiple_links() {
        let text = "Check out these files: https://drive.google.com/file/d/123/view and https://drive.google.com/open?id=456";
        let link = GoogleDriveService::detect_link(text);
        assert_eq!(link, Some("123".to_string()));
    }
}

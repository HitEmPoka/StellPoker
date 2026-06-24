# AWS CloudFront CDN Configuration

# CloudFront Distribution
resource "aws_cloudfront_distribution" "main" {
  count   = var.enable_cdn ? 1 : 0
  enabled = true

  origin {
    domain_name = var.enable_alb ? aws_lb.main[0].dns_name : ""
    origin_id   = "alb"

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "http-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }
  }

  default_cache_behavior {
    allowed_methods  = ["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"]
    cached_methods   = ["GET", "HEAD"]
    target_origin_id = "alb"

    forwarded_values {
      query_string = true
      headers      = ["Authorization", "Host", "Origin"]

      cookies {
        forward = "all"
      }
    }

    viewer_protocol_policy = "allow-all"
    default_ttl            = var.cdn_default_ttl
    max_ttl                = var.cdn_max_ttl
    compress               = true
  }

  cache_behavior {
    allowed_methods  = ["GET", "HEAD", "OPTIONS"]
    cached_methods   = ["GET", "HEAD"]
    path_pattern     = "/api/health"
    target_origin_id = "alb"

    forwarded_values {
      query_string = false

      cookies {
        forward = "none"
      }
    }

    viewer_protocol_policy = "allow-all"
    default_ttl            = 60
    max_ttl                = 300
  }

  cache_behavior {
    allowed_methods  = ["GET", "HEAD", "OPTIONS"]
    cached_methods   = ["GET", "HEAD"]
    path_pattern     = "/static/*"
    target_origin_id = "alb"

    forwarded_values {
      query_string = false

      cookies {
        forward = "none"
      }
    }

    viewer_protocol_policy = "redirect-to-https"
    default_ttl            = 86400
    max_ttl                = 31536000
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }

  logging_config {
    include_cookies = false
    bucket          = aws_s3_bucket.cdn_logs[0].bucket_regional_domain_name
    prefix          = "cloudfront-logs/"
  }

  tags = {
    Name = "${var.project_name}-cdn"
  }
}

# S3 Bucket for CloudFront Logs
resource "aws_s3_bucket" "cdn_logs" {
  count  = var.enable_cdn ? 1 : 0
  bucket = "${var.project_name}-cdn-logs-${data.aws_caller_identity.current.account_id}"

  lifecycle {
    prevent_destroy = false
  }

  tags = {
    Name = "${var.project_name}-cdn-logs"
  }
}

resource "aws_s3_bucket_versioning" "cdn_logs" {
  count  = var.enable_cdn ? 1 : 0
  bucket = aws_s3_bucket.cdn_logs[0].id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "cdn_logs" {
  count  = var.enable_cdn ? 1 : 0
  bucket = aws_s3_bucket.cdn_logs[0].id

  rule {
    id     = "delete-old-logs"
    status = "Enabled"

    expiration {
      days = 90
    }
  }

  rule {
    id     = "archive-to-glacier"
    status = "Enabled"

    transition {
      days          = 30
      storage_class = "GLACIER"
    }
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "cdn_logs" {
  count  = var.enable_cdn ? 1 : 0
  bucket = aws_s3_bucket.cdn_logs[0].id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

# CloudFront Invalidation (useful for cache busting)
resource "aws_cloudfront_invalidation" "main" {
  count           = var.enable_cdn ? 1 : 0
  distribution_id = aws_cloudfront_distribution.main[0].id
  paths           = ["/api/*"]

  lifecycle {
    create_before_destroy = true
  }
}

# CloudWatch Alarms for CloudFront
resource "aws_cloudwatch_metric_alarm" "cdn_4xx_errors" {
  count               = var.enable_monitoring && var.enable_cdn ? 1 : 0
  alarm_name          = "${var.project_name}-cdn-4xx-errors"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "4xxErrorRate"
  namespace           = "AWS/CloudFront"
  period              = 300
  statistic           = "Average"
  threshold           = 5
  alarm_description   = "Alert when CloudFront 4xx error rate exceeds 5%"

  dimensions = {
    DistributionId = aws_cloudfront_distribution.main[0].id
  }

  alarm_actions = var.alarm_email != "" ? [aws_sns_topic.alerts[0].arn] : []
}

resource "aws_cloudwatch_metric_alarm" "cdn_5xx_errors" {
  count               = var.enable_monitoring && var.enable_cdn ? 1 : 0
  alarm_name          = "${var.project_name}-cdn-5xx-errors"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "5xxErrorRate"
  namespace           = "AWS/CloudFront"
  period              = 300
  statistic           = "Average"
  threshold           = 1
  alarm_description   = "Alert when CloudFront 5xx error rate exceeds 1%"

  dimensions = {
    DistributionId = aws_cloudfront_distribution.main[0].id
  }

  alarm_actions = var.alarm_email != "" ? [aws_sns_topic.alerts[0].arn] : []
}

# Outputs
output "cdn_domain_name" {
  description = "CloudFront domain name"
  value       = var.enable_cdn ? aws_cloudfront_distribution.main[0].domain_name : ""
}

output "cdn_distribution_id" {
  description = "CloudFront distribution ID"
  value       = var.enable_cdn ? aws_cloudfront_distribution.main[0].id : ""
}

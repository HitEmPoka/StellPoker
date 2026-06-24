# Terraform Outputs

output "environment" {
  description = "Environment"
  value       = var.environment
}

output "aws_region" {
  description = "AWS region"
  value       = var.aws_region
}

output "vpc_id" {
  description = "VPC ID"
  value       = aws_vpc.main.id
}

output "ecs_cluster_name" {
  description = "ECS cluster name"
  value       = aws_ecs_cluster.main.name
}

output "alb_dns_name" {
  description = "ALB DNS name"
  value       = var.enable_alb ? aws_lb.main[0].dns_name : "ALB disabled"
}

output "rds_endpoint" {
  description = "RDS database endpoint"
  value       = aws_db_instance.main.endpoint
}

output "rds_address" {
  description = "RDS database address (without port)"
  value       = aws_db_instance.main.address
}

output "rds_port" {
  description = "RDS database port"
  value       = aws_db_instance.main.port
}

output "rds_database_name" {
  description = "RDS database name"
  value       = aws_db_instance.main.db_name
}

output "cdn_domain_name" {
  description = "CloudFront domain name"
  value       = var.enable_cdn ? aws_cloudfront_distribution.main[0].domain_name : "CDN disabled"
}

output "coordinator_service_name" {
  description = "Coordinator ECS service name"
  value       = aws_ecs_service.coordinator.name
}

output "cloudwatch_log_group" {
  description = "CloudWatch log group for ECS"
  value       = aws_cloudwatch_log_group.ecs.name
}

output "deployment_summary" {
  description = "Deployment summary"
  value = {
    environment          = var.environment
    region               = var.aws_region
    ecs_cluster          = aws_ecs_cluster.main.name
    alb_endpoint         = var.enable_alb ? aws_lb.main[0].dns_name : null
    database_endpoint    = aws_db_instance.main.endpoint
    cdn_enabled          = var.enable_cdn
    cdn_domain           = var.enable_cdn ? aws_cloudfront_distribution.main[0].domain_name : null
    monitoring_enabled   = var.enable_monitoring
    coordinator_tasks    = var.coordinator_desired_count
  }
}

output "connection_strings" {
  description = "Connection strings for services"
  value = {
    api_url   = var.enable_alb ? "http://${aws_lb.main[0].dns_name}" : null
    db_url    = "postgresql://${var.db_username}:****@${aws_db_instance.main.address}:${aws_db_instance.main.port}/${aws_db_instance.main.db_name}"
    cdn_url   = var.enable_cdn ? "https://${aws_cloudfront_distribution.main[0].domain_name}" : null
  }
  sensitive = true
}

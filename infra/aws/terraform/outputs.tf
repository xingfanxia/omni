output "omni_url" {
  description = "URL to access Omni application"
  value       = var.ssl_certificate_arn != "" ? "https://${module.loadbalancer.dns_name}" : "http://${module.loadbalancer.dns_name}"
}

output "alb_dns_name" {
  description = "Application Load Balancer DNS name"
  value       = module.loadbalancer.dns_name
}

output "database_endpoint" {
  description = "RDS PostgreSQL endpoint"
  value       = module.database.endpoint
}

output "redis_endpoint" {
  description = "Redis cluster endpoint"
  value       = module.cache.endpoint
}

output "ecs_cluster_name" {
  description = "ECS Cluster name"
  value       = module.compute.cluster_name
}

output "log_group_name" {
  description = "CloudWatch Log Group name"
  value       = module.monitoring.log_group_name
}

output "region" {
  description = "AWS region"
  value       = var.region
}

output "account_id" {
  description = "AWS Account ID"
  value       = data.aws_caller_identity.current.account_id
}

output "vpc_id" {
  description = "VPC ID"
  value       = module.networking.vpc_id
}

output "next_steps" {
  description = "What to do next"
  value       = <<-EOT
    Omni has been deployed successfully!

    📍 Access your application:
       ${var.ssl_certificate_arn != "" ? "https://${module.loadbalancer.dns_name}" : "http://${module.loadbalancer.dns_name}"}

    📊 Monitor your deployment:
       CloudWatch Logs: ${module.monitoring.log_group_name}
       ECS Cluster: ${module.compute.cluster_name}

    ${var.ssl_certificate_arn == "" ? "🔒 Setup HTTPS:\n       1. Request certificate in ACM\n       2. Add ssl_certificate_arn to terraform.tfvars\n       3. Run terraform apply\n       4. Update DNS to point to ALB" : ""}

    📚 View logs:
       aws logs tail ${module.monitoring.log_group_name} --follow

    🔧 Debug services:
       aws ecs execute-command \\
         --cluster ${module.compute.cluster_name} \\
         --task <task-id> \\
         --container omni-web \\
         --interactive \\
         --command "/bin/sh"
  EOT
}

# Storage outputs
output "content_bucket_name" {
  description = "S3 bucket name for content storage"
  value       = module.storage.content_bucket_name
}

output "batch_bucket_name" {
  description = "S3 bucket name for batch inference"
  value       = module.storage.batch_bucket_name
}

output "bedrock_batch_role_arn" {
  description = "IAM role ARN for Bedrock batch inference (set as EMBEDDING_BATCH_BEDROCK_ROLE_ARN)"
  value       = module.storage.bedrock_batch_role_arn
}

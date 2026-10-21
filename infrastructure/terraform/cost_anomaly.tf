# Cost Anomaly Detection for StellPoker Infrastructure
# Detects abnormal daily spend spikes and triggers alerts

terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

variable "cost_anomaly_threshold" {
  description = "Percentage increase from baseline to trigger alert (e.g., 25 = 25%)"
  type        = number
  default     = 25
}

variable "cost_anomaly_email" {
  description = "Email address for cost anomaly alerts"
  type        = string
}

variable "environment" {
  description = "Environment name"
  type        = string
  default     = "production"
}

# CloudWatch Log Group for cost anomaly events
resource "aws_cloudwatch_log_group" "cost_anomaly" {
  name              = "/stellpoker/cost-anomaly/${var.environment}"
  retention_in_days = 30

  tags = {
    Service     = "cost-monitoring"
    Environment = var.environment
  }
}

# SNS Topic for alerts
resource "aws_sns_topic" "cost_anomaly_alerts" {
  name              = "stellpoker-cost-anomaly-${var.environment}"
  kms_master_key_id = "alias/aws/sns"

  tags = {
    Service     = "cost-monitoring"
    Environment = var.environment
  }
}

resource "aws_sns_topic_subscription" "cost_anomaly_email" {
  topic_arn = aws_sns_topic.cost_anomaly_alerts.arn
  protocol  = "email"
  endpoint  = var.cost_anomaly_email
}

# Lambda function for cost anomaly detection
resource "aws_iam_role" "cost_anomaly_lambda" {
  name = "stellpoker-cost-anomaly-lambda-${var.environment}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "lambda.amazonaws.com"
      }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lambda_logs" {
  role       = aws_iam_role.cost_anomaly_lambda.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy" "cost_anomaly_policy" {
  name   = "cost-anomaly-policy"
  role   = aws_iam_role.cost_anomaly_lambda.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ce:GetCostAndUsage",
          "ce:GetDimensionValues"
        ]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "sns:Publish"
        ]
        Resource = aws_sns_topic.cost_anomaly_alerts.arn
      },
      {
        Effect = "Allow"
        Action = [
          "cloudwatch:PutMetricData"
        ]
        Resource = "*"
      }
    ]
  })
}

# Lambda function code
data "archive_file" "cost_anomaly_lambda_zip" {
  type        = "zip"
  output_path = "/tmp/cost_anomaly_lambda.zip"
  source {
    content  = file("${path.module}/../../services/cost-anomaly/lambda_function.py")
    filename = "lambda_function.py"
  }
}

resource "aws_lambda_function" "cost_anomaly_detector" {
  filename         = data.archive_file.cost_anomaly_lambda_zip.output_path
  function_name    = "stellpoker-cost-anomaly-${var.environment}"
  role             = aws_iam_role.cost_anomaly_lambda.arn
  handler          = "lambda_function.lambda_handler"
  source_code_hash = data.archive_file.cost_anomaly_lambda_zip.output_base64sha256
  runtime          = "python3.11"
  timeout          = 300

  environment {
    variables = {
      SNS_TOPIC_ARN              = aws_sns_topic.cost_anomaly_alerts.arn
      COST_ANOMALY_THRESHOLD     = var.cost_anomaly_threshold
      ENVIRONMENT                = var.environment
      CLOUDWATCH_LOG_GROUP       = aws_cloudwatch_log_group.cost_anomaly.name
    }
  }

  depends_on = [aws_iam_role_policy.cost_anomaly_policy]

  tags = {
    Service     = "cost-monitoring"
    Environment = var.environment
  }
}

# EventBridge rule to trigger daily at 9am UTC
resource "aws_cloudwatch_event_rule" "cost_anomaly_daily" {
  name                = "stellpoker-cost-anomaly-daily-${var.environment}"
  description         = "Trigger cost anomaly detection daily"
  schedule_expression = "cron(0 9 * * ? *)"

  tags = {
    Service     = "cost-monitoring"
    Environment = var.environment
  }
}

resource "aws_cloudwatch_event_target" "cost_anomaly_lambda" {
  rule      = aws_cloudwatch_event_rule.cost_anomaly_daily.name
  target_id = "CostAnomalyLambda"
  arn       = aws_lambda_function.cost_anomaly_detector.arn

  input = jsonencode({
    threshold = var.cost_anomaly_threshold
  })
}

resource "aws_lambda_permission" "allow_eventbridge" {
  statement_id  = "AllowExecutionFromEventBridge"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.cost_anomaly_detector.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.cost_anomaly_daily.arn
}

# CloudWatch Dashboard for cost monitoring
resource "aws_cloudwatch_dashboard" "cost_anomaly" {
  dashboard_name = "stellpoker-cost-anomaly-${var.environment}"

  dashboard_body = jsonencode({
    widgets = [
      {
        type = "metric"
        properties = {
          metrics = [
            ["AWS/Billing", "EstimatedCharges", { stat = "Average" }],
            ["StellPoker/Cost", "DailySpend", { stat = "Average" }],
            ["StellPoker/Cost", "AnomalyDetected", { stat = "Sum" }]
          ]
          period = 86400
          stat   = "Average"
          region = "us-east-1"
          title  = "Daily Cost Tracking"
        }
      },
      {
        type = "log"
        properties = {
          query   = "fields @timestamp, @message | filter @message like /anomaly/ | stats count() as anomaly_count by bin(5m)"
          region  = "us-east-1"
          title   = "Anomaly Detection Events"
        }
      }
    ]
  })
}

output "cost_anomaly_sns_topic" {
  value       = aws_sns_topic.cost_anomaly_alerts.arn
  description = "SNS topic ARN for cost anomaly alerts"
}

output "cost_anomaly_lambda_function" {
  value       = aws_lambda_function.cost_anomaly_detector.function_name
  description = "Lambda function name for cost anomaly detection"
}

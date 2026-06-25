resource "aws_budgets_budget" "monthly_budget" {
  name              = "monthly-total-budget"
  budget_type       = "COST"
  limit_amount      = "100" # Update this to your target monthly limit
  limit_unit        = "USD"
  time_unit         = "MONTHLY"

  # Notification for 50%, 80%, and 100%
  dynamic "notification" {
    for_each = [0.5, 0.8, 1.0]
    content {
      comparison_operator        = "GREATER_THAN"
      threshold                  = notification.value * 100
      threshold_type             = "PERCENTAGE"
      notification_type          = "ACTUAL"
      subscriber_email_addresses = ["team@stellpoker.com"] # Update with your team email
    }
  }
}

# Optional: Weekly Cost and Usage Report (CUR)
resource "aws_cur_report_definition" "weekly_report" {
  report_name                = "weekly-cost-report"
  time_unit                  = "HOURLY" # AWS CUR granularity
  format                     = "textORcsv"
  compression                = "GZIP"
  additional_schema_elements = ["RESOURCES"]
  s3_bucket                  = "your-reports-bucket-name" # Must exist
  s3_region                  = "us-east-1"
  s3_prefix                  = "reports"
}
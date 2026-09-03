resource "aws_s3_bucket" "events" {
  bucket = "analytics-events"
}

resource "aws_s3_bucket_lifecycle_configuration" "events" {
  bucket = aws_s3_bucket.events.id

  rule {
    id     = "expire"
    status = "Enabled"

    # Governance retention: one year, agreed with data governance.
    expiration {
      days = 365
    }
  }
}

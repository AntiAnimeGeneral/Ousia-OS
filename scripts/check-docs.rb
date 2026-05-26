#!/usr/bin/env ruby
# frozen_string_literal: true

require "pathname"

ROOT = Pathname.new(__dir__).parent.expand_path
DESIGN_DIR = ROOT / "design"
CORE_DIR = DESIGN_DIR / "core"

errors = []
warnings = []

unless DESIGN_DIR.directory?
  warn "design directory not found: #{DESIGN_DIR}"
  exit 1
end

markdown_files = Dir.glob((DESIGN_DIR / "**/*.md").to_s).map { |path| Pathname.new(path) }.sort

markdown_files.each do |file|
  text = file.read
  text.scan(/\[[^\]]*\]\(([^)]+)\)/).flatten.each do |raw_target|
    target = raw_target.strip.split(/\s+/, 2).first
    next if target.nil? || target.empty?
    next if target.start_with?("http://", "https://", "mailto:", "#")

    path = target.split("#", 2).first
    next if path.empty? || !path.end_with?(".md")

    full_path = (file.dirname / path).expand_path
    next if full_path.file?

    errors << "broken markdown link: #{file.relative_path_from(ROOT)} -> #{target}"
  end
end

core_files = Dir.glob((CORE_DIR / "*.md").to_s).map { |path| Pathname.new(path) }.sort
numbered_core_files = core_files.select { |file| file.basename.to_s.match?(/\A\d{2}-/) }
actual_numbers = numbered_core_files.map { |file| file.basename.to_s[/\A\d{2}/].to_i }
expected_numbers = (0...numbered_core_files.length).to_a

if actual_numbers != expected_numbers
  errors << "core chapter numbers are not continuous: expected #{expected_numbers.map { |n| format('%02d', n) }.join(', ')}, got #{actual_numbers.map { |n| format('%02d', n) }.join(', ')}"
end

numbered_core_files.each do |file|
  filename_number = file.basename.to_s[/\A\d{2}/]
  first_heading = file.each_line.find { |line| line.start_with?("# ") }

  if first_heading.nil?
    errors << "missing H1 heading: #{file.relative_path_from(ROOT)}"
    next
  end

  heading_number = first_heading[/\A#\s+(\d{2})\b/, 1]
  if heading_number.nil?
    errors << "H1 heading does not start with a two-digit chapter number: #{file.relative_path_from(ROOT)}"
  elsif heading_number != filename_number
    errors << "filename/H1 number mismatch: #{file.relative_path_from(ROOT)} has H1 #{heading_number}"
  end
end

stale_core_refs = {
  "01-package-cell.md" => "08-package-cell.md",
  "02-capsule-and-capability.md" => "01-capsule-and-capability.md",
  "03-service-graph.md" => "06-service-graph.md",
  "04-data-and-filesystem.md" => "07-data-and-filesystem.md",
  "05-pager-and-memory.md" => "03-pager-and-memory.md",
  "06-compute-and-scheduling.md" => "05-compute-and-scheduling.md",
  "07-driver-and-kernel.md" => "04-driver-and-kernel.md",
  "08-communication-fabric.md" => "02-communication-fabric.md"
}

markdown_files.each do |file|
  text = file.read
  stale_core_refs.each do |old_name, new_name|
    next unless text.include?(old_name)

    warnings << "possible stale core reference in #{file.relative_path_from(ROOT)}: #{old_name} should usually be #{new_name}"
  end
end

if errors.empty? && warnings.empty?
  puts "OK: documentation checks passed"
elsif errors.empty?
  warnings.each { |message| warn "WARN: #{message}" }
  puts "OK: documentation checks passed with warnings"
else
  warnings.each { |message| warn "WARN: #{message}" }
  errors.each { |message| warn "ERROR: #{message}" }
  exit 1
end

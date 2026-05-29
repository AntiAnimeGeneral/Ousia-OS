#!/usr/bin/env ruby
# frozen_string_literal: true

require "pathname"

ROOT = Pathname.new(__dir__).parent.expand_path
DESIGN_DIR = ROOT / "design"
CORE_DIR = DESIGN_DIR / "core"
TOPICS_DIR = DESIGN_DIR / "topics"
TARGET_FILE = DESIGN_DIR / "target.md"

errors = []
warnings = []

unless DESIGN_DIR.directory?
  warn "design directory not found: #{DESIGN_DIR}"
  exit 1
end

markdown_files = Dir.glob((DESIGN_DIR / "**/*.md").to_s).map { |path| Pathname.new(path) }.sort
markdown_basenames = markdown_files.map { |file| file.basename.to_s }.uniq

target_sections = []
if TARGET_FILE.file?
  target_sections = TARGET_FILE.each_line.map do |line|
    line[/\A\#{2,6}\s+(\d+(?:\.\d+)*)(?:[\.．])?\s+/, 1]
  end.compact
else
  errors << "missing target document: #{TARGET_FILE.relative_path_from(ROOT)}"
end

markdown_files.each do |file|
  text = file.read
  text.scan(/\[([^\]]*)\]\(([^)]+)\)/).each do |link_text, raw_target|
    target = raw_target.strip.split(/\s+/, 2).first
    next if target.nil? || target.empty?
    next if target.start_with?("http://", "https://", "mailto:", "#")

    path = target.split("#", 2).first
    next if path.empty? || !path.end_with?(".md")

    full_path = (file.dirname / path).expand_path
    if full_path.file?
      target_basename = Pathname.new(path).basename.to_s
      displayed_target = link_text.strip.delete_prefix("`").delete_suffix("`")
      if displayed_target.match?(%r{\A(?:\.\.?/)?(?:[^/\]]+/)*[^/\]]+\.md\z})
        displayed_basename = Pathname.new(displayed_target).basename.to_s
        if displayed_basename != target_basename
          errors << "markdown link text does not match target filename: #{file.relative_path_from(ROOT)} has [#{link_text}] -> #{target_basename}"
        end
      end

      next
    end

    errors << "broken markdown link: #{file.relative_path_from(ROOT)} -> #{target}"
  end
end

numbered_markdown_files = markdown_files.select { |file| file.basename.to_s.match?(/\A\d{2}-/) }
numbered_markdown_files.each do |file|
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

sequential_chapter_dirs = [CORE_DIR, TOPICS_DIR]
sequential_chapter_dirs.each do |dir|
  next unless dir.directory?

  files = Dir.glob((dir / "*.md").to_s).map { |path| Pathname.new(path) }.sort
  numbered_files = files.select { |file| file.basename.to_s.match?(/\A\d{2}-/) }
  actual_numbers = numbered_files.map { |file| file.basename.to_s[/\A\d{2}/].to_i }
  expected_numbers = (0...numbered_files.length).to_a
  next if actual_numbers == expected_numbers

  errors << "#{dir.relative_path_from(ROOT)} chapter numbers are not continuous: expected #{expected_numbers.map { |n| format('%02d', n) }.join(', ')}, got #{actual_numbers.map { |n| format('%02d', n) }.join(', ')}"
end

markdown_files.each do |file|
  text = file.read

  text.scan(/(?<![[:alnum:]_\/.-])(\d{2}-[[:alnum:]_.-]+\.md)(?![[:alnum:]_.-])/) do |match|
    filename = match.first
    next if markdown_basenames.include?(filename)

    errors << "unknown numbered markdown filename reference in #{file.relative_path_from(ROOT)}: #{filename}"
  end

  text.each_line.with_index(1) do |line, line_number|
    next unless line.include?("target.md") && line.include?("§")

    line.scan(/§\s*(\d+(?:\.\d+)*)/) do |match|
      section = match.first
      next if target_sections.include?(section)

      errors << "stale target.md section reference in #{file.relative_path_from(ROOT)}:#{line_number}: §#{section}"
    end
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

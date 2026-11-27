#!/usr/bin/env ruby
# frozen_string_literal: true

# Prepare cargo-dist's LogBrew formula for strict Homebrew validation.

FORMULA_SIZE_LIMIT = 128 * 1024
EXPECTED_DESCRIPTION = "Developer-first observability command-line interface"
ACCEPTED_SOURCE_DESCRIPTIONS = [
  EXPECTED_DESCRIPTION,
  "Public command-line interface for LogBrew.",
].freeze
EXPECTED_TARGETS = %w[
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-apple-darwin
  x86_64-unknown-linux-gnu
].freeze
FORMULA_TEST = <<'RUBY'
  test do
    assert_match version.to_s, shell_output("#{bin}/logbrew version")
  end
RUBY
# cargo-dist writes an alias table and 2 helpers that read it. Every alias is
# empty, and Homebrew's style audit refuses a helper with only 1 caller.
EMPTY_BINARY_ALIASES = /^  BINARY_ALIASES = \{\n(?:    "[a-z0-9_-]+": \{\},?\n)+  \}\n\n/
ALIAS_HELPERS = [
  /^  def target_triple\n(?:(?:    .*)?\n)*?  end\n\n/,
  /^  def install_binary_aliases!\n(?:(?:    .*)?\n)*?  end\n\n/,
  /\n\n    install_binary_aliases!\n/,
].freeze

def fail_closed
  warn "Homebrew formula preparation failed."
  exit 1
end

fail_closed unless ARGV.length == 2

formula_path = File.expand_path(ARGV.fetch(0))
expected_version = ARGV.fetch(1)
fail_closed unless File.basename(formula_path) == "logbrew.rb"
fail_closed unless expected_version.match?(/\A[0-9]+\.[0-9]+\.[0-9]+\z/)

temporary_path = nil

begin
  metadata = File.lstat(formula_path)
  fail_closed unless metadata.file? && !metadata.symlink?
  fail_closed unless metadata.size.positive? && metadata.size <= FORMULA_SIZE_LIMIT

  formula = File.binread(formula_path)
  formula.force_encoding(Encoding::UTF_8)
  fail_closed unless formula.valid_encoding?
  fail_closed if formula.include?("\0") || formula.include?("\r")
  fail_closed unless formula.end_with?("\n")
  fail_closed unless formula.scan(/^class Logbrew < Formula$/).length == 1
  description_lines = formula.scan(/^  desc "([^"]+)"$/).flatten
  fail_closed unless description_lines.length == 1
  fail_closed unless ACCEPTED_SOURCE_DESCRIPTIONS.include?(description_lines.first)
  fail_closed unless formula.scan(/^  homepage "https:\/\/logbrew\.co"$/).length == 1
  fail_closed unless formula.scan(/^  license "MIT"$/).length == 1
  fail_closed if formula.include?("\n  test do\n")

  version_lines = formula.scan(/^  version "([^"]+)"$/)
  fail_closed unless version_lines == [[expected_version]]

  release_urls = formula.scan(
    %r{^      url "https://github\.com/LogBrewCo/cli/releases/download/v([^/]+)/logbrew-cli-([a-z0-9_-]+)\.tar\.xz"$},
  )
  fail_closed unless release_urls.length == EXPECTED_TARGETS.length
  fail_closed unless release_urls.all? { |version, _target| version == expected_version }
  fail_closed unless release_urls.map(&:last).sort == EXPECTED_TARGETS.sort

  checksums = formula.scan(/^      sha256 "([0-9a-f]{64})"$/).flatten
  fail_closed unless checksums.length == EXPECTED_TARGETS.length
  fail_closed unless checksums.uniq.length == checksums.length

  class_closing = "\nend\n"
  fail_closed unless formula.end_with?(class_closing)

  prepared = formula.sub(%(  version "#{expected_version}"\n), "")
  fail_closed if prepared == formula
  prepared = prepared.sub(
    %(  desc "#{description_lines.first}"\n),
    %(  desc "#{EXPECTED_DESCRIPTION}"\n),
  )
  [EMPTY_BINARY_ALIASES, *ALIAS_HELPERS].each do |pattern|
    fail_closed unless prepared.scan(pattern).length == 1
    prepared = prepared.sub(pattern) { |match| match.start_with?("\n") ? "\n" : "" }
  end
  prepared = prepared.delete_suffix(class_closing)
  prepared = "#{prepared}\n\n#{FORMULA_TEST}end\n"

  directory = File.dirname(formula_path)
  temporary_path = File.join(
    directory,
    ".#{File.basename(formula_path)}.prepare-#{Process.pid}",
  )
  flags = File::WRONLY | File::CREAT | File::EXCL
  File.open(temporary_path, flags, metadata.mode & 0o777) do |handle|
    handle.write(prepared)
    handle.flush
    handle.fsync
  end
  File.rename(temporary_path, formula_path)
  temporary_path = nil
rescue StandardError
  fail_closed
ensure
  if temporary_path && File.file?(temporary_path) && !File.symlink?(temporary_path)
    File.delete(temporary_path)
  end
end

puts "Homebrew formula prepared."

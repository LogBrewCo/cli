#!/usr/bin/env ruby
# frozen_string_literal: true

require "minitest/autorun"
require "open3"
require "rbconfig"
require "tmpdir"

ROOT = File.expand_path("..", __dir__)
SUBJECT = File.join(ROOT, "scripts", "prepare-homebrew-formula.rb")

class PrepareHomebrewFormulaTest < Minitest::Test
  VERSION = "1.2.3"

  def generated_formula
    <<~RUBY
      class Logbrew < Formula
        desc "Developer-first observability command-line interface"
        homepage "https://logbrew.co"
        version "#{VERSION}"
        if OS.mac?
          if Hardware::CPU.arm?
            url "https://github.com/LogBrewCo/cli/releases/download/v#{VERSION}/logbrew-cli-aarch64-apple-darwin.tar.xz"
            sha256 "#{"a" * 64}"
          end
          if Hardware::CPU.intel?
            url "https://github.com/LogBrewCo/cli/releases/download/v#{VERSION}/logbrew-cli-x86_64-apple-darwin.tar.xz"
            sha256 "#{"b" * 64}"
          end
        end
        if OS.linux?
          if Hardware::CPU.arm?
            url "https://github.com/LogBrewCo/cli/releases/download/v#{VERSION}/logbrew-cli-aarch64-unknown-linux-gnu.tar.xz"
            sha256 "#{"c" * 64}"
          end
          if Hardware::CPU.intel?
            url "https://github.com/LogBrewCo/cli/releases/download/v#{VERSION}/logbrew-cli-x86_64-unknown-linux-gnu.tar.xz"
            sha256 "#{"d" * 64}"
          end
        end
        license "MIT"

        BINARY_ALIASES = {
          "aarch64-apple-darwin": {},
          "aarch64-unknown-linux-gnu": {},
          "x86_64-apple-darwin": {},
          "x86_64-pc-windows-gnu": {},
          "x86_64-unknown-linux-gnu": {}
        }

        def target_triple
          cpu = Hardware::CPU.arm? ? "aarch64" : "x86_64"
          os = OS.mac? ? "apple-darwin" : "unknown-linux-gnu"

          "\#{cpu}-\#{os}"
        end

        def install_binary_aliases!
          BINARY_ALIASES[target_triple.to_sym].each do |source, dests|
            dests.each do |dest|
              bin.install_symlink bin/source.to_s => dest
            end
          end
        end

        def install
          if OS.mac? && Hardware::CPU.arm?
            bin.install "logbrew"
          end
          if OS.mac? && Hardware::CPU.intel?
            bin.install "logbrew"
          end
          if OS.linux? && Hardware::CPU.arm?
            bin.install "logbrew"
          end
          if OS.linux? && Hardware::CPU.intel?
            bin.install "logbrew"
          end

          install_binary_aliases!
        end
      end
    RUBY
  end

  def with_formula(source = generated_formula)
    Dir.mktmpdir("logbrew-homebrew-formula-") do |directory|
      path = File.join(directory, "logbrew.rb")
      File.write(path, source, mode: "wb")
      yield path, directory
    end
  end

  def prepare(path, version = VERSION)
    Open3.capture3(RbConfig.ruby, SUBJECT, path, version)
  end

  def assert_closed_failure(source, version = VERSION)
    with_formula(source) do |path, directory|
      stdout, stderr, status = prepare(path, version)
      refute status.success?
      assert_empty stdout
      assert_equal "Homebrew formula preparation failed.\n", stderr
      assert_empty Dir.glob(File.join(directory, ".logbrew.rb.prepare-*"))
    end
  end

  def test_prepares_one_strict_formula_without_changing_release_identity
    with_formula do |path, _directory|
      stdout, stderr, status = prepare(path)
      assert status.success?
      assert_equal "Homebrew formula prepared.\n", stdout
      assert_empty stderr

      prepared = File.read(path, encoding: Encoding::UTF_8)
      refute_includes prepared, %(  version "#{VERSION}")
      assert_includes prepared, "/releases/download/v#{VERSION}/"
      assert_equal 4, prepared.scan("/releases/download/v#{VERSION}/").length
      assert_equal 1, prepared.scan("\n  test do\n").length
      assert_includes(
        prepared,
        %(assert_match version.to_s, shell_output("\#{bin}/logbrew version")),
      )

      syntax_output, syntax_error, syntax_status = Open3.capture3(
        RbConfig.ruby,
        "-c",
        path,
      )
      assert syntax_status.success?
      assert_equal "Syntax OK\n", syntax_output
      assert_empty syntax_error
    end
  end

  def test_rejects_release_version_substitution
    assert_closed_failure(generated_formula, "1.2.4")
  end

  def test_normalizes_the_released_legacy_description
    source = generated_formula.sub(
      "Developer-first observability command-line interface",
      "Public command-line interface for LogBrew.",
    )
    with_formula(source) do |path, _directory|
      _stdout, _stderr, status = prepare(path)
      assert status.success?
      prepared = File.read(path, encoding: Encoding::UTF_8)
      assert_includes(
        prepared,
        'desc "Developer-first observability command-line interface"',
      )
      refute_includes prepared, "Public command-line interface for LogBrew."
    end
  end

  def test_rejects_duplicate_version_lines
    source = generated_formula.sub(
      %(  version "#{VERSION}"\n),
      %(  version "#{VERSION}"\n  version "#{VERSION}"\n),
    )
    assert_closed_failure(source)
  end

  def test_rejects_target_substitution
    source = generated_formula.sub(
      "aarch64-unknown-linux-gnu",
      "i686-unknown-linux-gnu",
    )
    assert_closed_failure(source)
  end

  def test_rejects_existing_test_block
    source = generated_formula.sub(
      "\nend\n",
      "\n  test do\n    system \"false\"\n  end\nend\n",
    )
    assert_closed_failure(source)
  end

  def test_rejects_symlinked_formula_without_changing_the_target
    Dir.mktmpdir("logbrew-homebrew-formula-link-") do |directory|
      target = File.join(directory, "target.rb")
      path = File.join(directory, "logbrew.rb")
      File.write(target, generated_formula, mode: "wb")
      File.symlink(target, path)

      stdout, stderr, status = prepare(path)
      refute status.success?
      assert_empty stdout
      assert_equal "Homebrew formula preparation failed.\n", stderr
      assert_equal generated_formula, File.read(target, encoding: Encoding::UTF_8)
    end
  end
end

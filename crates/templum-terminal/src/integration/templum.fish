# Templum's shell integration for fish, started with `--init-command 'source …'` this file. It
# tells Templum when a command starts and ends (OSC 133 C and D, with the exit code) and the
# current folder (OSC 7), and changes nothing else.

if status is-interactive; and not set -q __templum_installed
    set -g __templum_installed 1
    function __templum_preexec --on-event fish_preexec
        printf '\e]133;C\a'
    end
    function __templum_postexec --on-event fish_postexec
        printf '\e]133;D;%s\a' $status
    end
    function __templum_prompt --on-event fish_prompt
        printf '\e]7;file://%s%s\a' $hostname (string replace -a '%' '%25' -- $PWD)
    end
end

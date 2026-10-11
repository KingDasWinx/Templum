# Templum's shell integration for bash, started with `--init-file` this file. It tells Templum
# when a command starts and ends (OSC 133 C and D, with the exit code) and the current folder
# (OSC 7), and changes nothing else. Bash 4.4 or newer reports commands; any bash, the folder.
# The startup files part follows VS Code's shellIntegration-bash.sh (MIT, (c) Microsoft).

# --init-file took the place of the startup files: read the ones bash would have read.
if [[ -n "${TEMPLUM_SHELL_LOGIN-}" ]]; then
    builtin unset TEMPLUM_SHELL_LOGIN
    [[ -r /etc/profile ]] && builtin . /etc/profile
    for __templum_file in ~/.bash_profile ~/.bash_login ~/.profile; do
        if [[ -r "$__templum_file" ]]; then
            builtin . "$__templum_file"
            break
        fi
    done
    builtin unset __templum_file
else
    [[ -r /etc/bash.bashrc ]] && builtin . /etc/bash.bashrc
    [[ -r ~/.bashrc ]] && builtin . ~/.bashrc
fi

if [[ -z "${__templum_installed-}" ]]; then
    __templum_installed=1

    # First in PROMPT_COMMAND, for the command's status; it gives it back to what runs next.
    __templum_precmd() {
        builtin local status=$?
        builtin printf '\e]133;D;%s\a\e]7;file://%s%s\a' "$status" "${HOSTNAME-}" "${PWD//\%/%25}"
        # PS0 marks a command's start; a prompt theme may have set it again since.
        [[ "${PS0-}" == *'\e]133;C\a'* ]] || PS0="${PS0-}"'\e]133;C\a'
        builtin return "$status"
    }
    PROMPT_COMMAND="__templum_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi

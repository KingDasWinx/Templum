# Templum's shell integration for zsh, found through ZDOTDIR. It tells Templum when a command
# starts and ends (OSC 133 C and D, with the exit code) and the current folder (OSC 7), and
# changes nothing else.

# The user's ZDOTDIR back first: zsh reads .zprofile, .zshrc and .zlogin from it.
if (( ${+TEMPLUM_ZDOTDIR} )); then
    ZDOTDIR=$TEMPLUM_ZDOTDIR
    builtin unset TEMPLUM_ZDOTDIR
else
    builtin unset ZDOTDIR
fi
# The user's .zshenv, which may set ZDOTDIR itself.
[[ -r ${ZDOTDIR:-$HOME}/.zshenv ]] && builtin source ${ZDOTDIR:-$HOME}/.zshenv

if [[ -o interactive && -z ${__templum_installed-} ]]; then
    __templum_installed=1
    __templum_precmd() {
        builtin local ret=$?
        builtin printf '\e]133;D;%s\a\e]7;file://%s%s\a' $ret "$HOST" "${PWD//\%/%25}"
        builtin return $ret
    }
    __templum_preexec() {
        builtin printf '\e]133;C\a'
    }
    builtin autoload -Uz add-zsh-hook
    # Before the .zshrc's hooks: the first precmd sees the command's status.
    add-zsh-hook precmd __templum_precmd
    add-zsh-hook preexec __templum_preexec
fi

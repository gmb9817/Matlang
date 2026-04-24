root = tempname();
mkdir(root);
old = cd(root);

mkdir alpha
mkdir parent child
mkdir helpers
mkdir +pkg
mkdir('@MiniCounter');

fid = fopen('note.txt', 'w');
fprintf(fid, 'hello');
fclose(fid);

helper_fid = fopen(fullfile('helpers', 'helpervalue.m'), 'w');
fprintf(helper_fid, 'function y = helpervalue(x)\n');
fprintf(helper_fid, '%% HELPERVALUE CommandFormHelperSummary.\n');
fprintf(helper_fid, 'y = strcmp(x, ''1'');\n');
fprintf(helper_fid, 'end\n');
fclose(helper_fid);

pkg_fid = fopen(fullfile('+pkg', 'tool.m'), 'w');
fprintf(pkg_fid, 'function y = tool(x)\n');
fprintf(pkg_fid, '%% TOOL CommandFormPkgSummary.\n');
fprintf(pkg_fid, 'y = strcmp(x, ''1'');\n');
fprintf(pkg_fid, 'end\n');
fclose(pkg_fid);

class_fid = fopen(fullfile('@MiniCounter', 'MiniCounter.m'), 'w');
fprintf(class_fid, 'classdef MiniCounter\n');
fprintf(class_fid, '%% MINICOUNTER CommandFormClassSummary.\n');
fprintf(class_fid, 'methods\n');
fprintf(class_fid, 'function obj = MiniCounter(varargin)\n');
fprintf(class_fid, 'end\n');
fprintf(class_fid, 'end\n');
fprintf(class_fid, 'end\n');
fclose(class_fid);

copyfile note.txt note_copy.txt
movefile note_copy.txt moved.txt

alpha_exists = exist('alpha', 'dir');
child_exists = exist(fullfile('parent', 'child'), 'dir');
moved_exists_before_delete = exist('moved.txt', 'file');
helper_exists_before = exist('helpervalue', 'file');

addpath helpers -end
helper_exists_after_add = exist('helpervalue', 'file');
helper_call = helpervalue 1
which_txt = which helpervalue
which_has_helper_file = contains(which_txt, 'helpervalue.m');
which_all = which helpervalue -all
which_all_count = numel(which_all);
help_txt = help helpervalue
help_has_summary = contains(help_txt, 'CommandFormHelperSummary');
help helpervalue
doc helpervalue
lookfor CommandFormHelperSummary
current_info = what .
current_path_matches = strcmp(current_info.path, root);
current_class_name = current_info.classes{1};
current_package_name = current_info.packages{1};
what_info = what helpers
what_entry = what_info(1);
what_name = what_entry.m{1};
pkg_call = pkg.tool 1
pkg_which_txt = which pkg.tool
pkg_which_has_file = contains(pkg_which_txt, 'tool.m');
pkg_which_all = which pkg.tool -all
pkg_which_all_count = numel(pkg_which_all);
pkg_help_txt = help pkg.tool
pkg_help_has_summary = contains(pkg_help_txt, 'CommandFormPkgSummary');
help pkg.tool
doc pkg.tool
pkg_info = what pkg
pkg_entry = pkg_info(1);
pkg_name = pkg_entry.m{1};
class_which_txt = which MiniCounter
class_which_has_file = contains(class_which_txt, 'MiniCounter.m');
class_which_all = which MiniCounter -all
class_which_all_count = numel(class_which_all);
class_help_txt = help MiniCounter
class_help_has_summary = contains(class_help_txt, 'CommandFormClassSummary');
help MiniCounter
doc MiniCounter
class_info = what MiniCounter
class_entry = class_info(1);
class_name = class_entry.m{1};
pwd_text = pwd
pwd_ischar = ischar(pwd_text);
dir_txt = dir *.txt
dir_txt_count = numel(dir_txt);
ls_txt = ls *.txt
ls_has_note = contains(ls_txt, 'note.txt');
rmpath helpers
helper_exists_after_remove = exist('helpervalue', 'file');

delete moved.txt
rmdir alpha
rmdir parent s

moved_exists_after_delete = exist('moved.txt', 'file');
alpha_exists_after_rmdir = exist('alpha', 'dir');
parent_exists_after_rmdir = exist('parent', 'dir');
listing = {dir().name};

cd(old);
rmdir(root, 's');
clear root old fid helper_fid pkg_fid class_fid pwd_text dir_txt ls_txt which_txt which_all help_txt current_info what_info what_entry pkg_which_txt pkg_which_all pkg_help_txt pkg_info pkg_entry class_which_txt class_which_all class_help_txt class_info class_entry

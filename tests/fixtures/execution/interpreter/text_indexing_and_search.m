s = 'alpha';
a = s(2);
b = s(2:4);
c = s([1 5]);
d = s(:);
s(2) = 'Z';
t = 'xy';
t(3) = 'z';
ta = 'xy';
ta(5) = 'z';
tb = double(ta);
q = 'abc';
q(2:3) = 'YZ';
u = 'abcd';
u([2 4]) = [];
v = contains("alphabet", "pha");
w = contains('alpha', 'zz');
x = strlength("alphabet");
y = strcmp(b, 'lph');
z = contains alphabet pha
aa = strfind('banana', 'na');
ab = strfind("alphabet", "a");
ac = strfind({'banana', "cabana"}, 'ba');
strfind_force = strfind('banana', 'na', 'ForceCellOutput', true);
strfind_force_string = strfind("alphabet", "a", 'ForceCellOutput', 1);
strfind_empty = strfind('banana', '');
strfind_empty_force = strfind('banana', '', 'ForceCellOutput', true);
try
  strfind(["ab"; "ba"], ["a"; "b"]);
catch err2
  strfind_pattern_id = err2.identifier;
  strfind_pattern_msg = err2.message;
end
ad = reshape('abcdefgh', [2 2 2]);
ad(:, :, 4) = ['m' 'n'; 'o' 'p'];
ad1 = ad(:, :, 1);
ad2 = ad(:, :, 2);
ad3 = ad(:, :, 3);
ad4 = ad(:, :, 4);
ad_size = size(ad);

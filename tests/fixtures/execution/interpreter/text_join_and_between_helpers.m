a = strip('  alpha  ');
b = strjoin({'a', 'b', 'c'}, "-");
c = extractBetween("pre<mid>post", '<', ">");
d = replaceBetween('pre<mid>post', "<", '>', "X");
e = strjoin(split("alpha-beta-gamma", '-'), "/");
f = strip(replaceBetween("  pre[mid]post  ", "[", "]", "Q"));
g = extractBetween(["Edgar Allen Poe"; "Louisa May Alcott"], [7; 8], [11; 10]);
h = extractBetween("Hello, world!", "w", "d", "Boundaries", "inclusive");
i = extractBetween({'pre<mid>post', 'alpha<beta>gamma'}, '<', '>');
j = replaceBetween({'pre<mid>post', 'alpha[beta]gamma'}, [5, 7], [7, 10], 'X');
k = strsplit('Hello,,,world', ',');
[l, m] = strsplit("alpha,beta;gamma", {",", ";"});
n = strsplit("left  right", ' ');
o = strsplit('a--b', '-', 'CollapseDelimiters', false);
[p, q] = strtok('  alpha beta');
[r, s] = strtok("left/right", '/');
[t, u] = strsplit('1.21m/s1.985m/s 1.955 m/s2.015 m/s 1.885m/s', '\s*m/s\s*', 'DelimiterType', 'RegularExpression');
[v, w] = strsplit('The rain in Spain stays mainly in the plain.', {'\s', 'ain'}, 'CollapseDelimiters', false, 'DelimiterType', 'RegularExpression');
try
  strsplit(['ab'; 'cd'], ',');
catch err1
  aad = err1.identifier;
  aae = err1.message;
end
try
  strtok(['ab'; 'cd']);
catch err2
  ad = err2.identifier;
  ae = err2.message;
end
x = extractBetween(['<a>'; '<b>'], '<', '>');
y = extractBetween(cat(3, ['<a>'; '<b>'], ['<c>'; '<d>']), '<', '>');
z = extractBetween(cat(3, ['ab'; 'cd'], ['ef'; 'gh']), 1, 2);
aa = strjoin(["ab" "cd"], "/");
try
  strjoin(reshape(["ab"; "cd"; "ef"; "gh"], [2 1 2]), "/");
catch err
  ab = err.identifier;
  ac = err.message;
end

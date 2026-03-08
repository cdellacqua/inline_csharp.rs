use inline_java::{ct_java, java};

// java = "..." : single system property passed to the JVM

#[test]
fn java_runtime_single_java_arg() {
	let val: Result<String, _> = java! {
		java = "-Dinline.test=hello",
		static String run() {
			return System.getProperty("inline.test");
		}
	};
	assert_eq!(val, Ok("hello".to_string()));
}

#[test]
fn java_runtime_single_java_arg_with_spaces() {
	let val: Result<String, _> = java! {
		java = "-Dinline.test='hello world'",
		static String run() {
			return System.getProperty("inline.test");
		}
	};
	assert_eq!(val, Ok("hello world".to_string()));
}

// java = "..." : multiple args are split on whitespace

#[test]
fn java_runtime_multiple_java_args() {
	let val: Result<String, _> = java! {
		java = "-Da=foo -Db=bar",
		static String run() {
			return System.getProperty("a") + ":" + System.getProperty("b");
		}
	};
	assert_eq!(val, Ok("foo:bar".to_string()));
}

// classpath JAR via $INLINE_JAVA_CP

#[test]
fn java_runtime_javac_classpath_jar() {
	let val: Result<String, _> = java! {
		javac = "-cp \"demo.jar\"",
		java = "-cp $INLINE_JAVA_CP:demo.jar",
		import com.example.demo.*;
		static String run() {
			return new HelloWorld().greet();
		}
	};
	assert_eq!(val, Ok("Hello, World!".to_string()));
}

#[test]
fn java_runtime_javac_classpath_jar_long_arg_name() {
	let val: Result<String, _> = java! {
		javac = "-classpath \"demo.jar\"",
		java = "-classpath $INLINE_JAVA_CP:demo.jar",
		import com.example.demo.*;
		static String run() {
			return new HelloWorld().greet();
		}
	};
	assert_eq!(val, Ok("Hello, World!".to_string()));
}

// javac = "..." : sourcepath lets javac resolve project Java files

#[test]
fn java_runtime_javac_sourcepath() {
	let val: Result<String, _> = java! {
		javac = "-sourcepath .",
		import com.example.demo.*;
		static String run() {
			return new HelloWorld().greet();
		}
	};
	assert_eq!(val, Ok("Hello, World!".to_string()));
}

// both opts together

#[test]
fn java_runtime_javac_and_java_args() {
	let val: Result<String, _> = java! {
		javac = "-sourcepath .",
		java = "-Dinline.combined=yes",
		import com.example.demo.*;
		static String run() {
			return new HelloWorld().greet() + "|" + System.getProperty("inline.combined");
		}
	};
	assert_eq!(val, Ok("Hello, World!|yes".to_string()));
}

// ct_java! with java = "..."

const CT_JAVA_ARG: &str = ct_java! {
	java = "-Dinline.ct=compile-time",
	static String run() {
		return System.getProperty("inline.ct");
	}
};

#[test]
fn ct_java_java_arg() {
	assert_eq!(CT_JAVA_ARG, "compile-time");
}

// ct_java! with javac = "..."

const CT_JAVAC_SOURCEPATH: &str = ct_java! {
	javac = "-sourcepath ./inline_java_demo",
	import com.example.demo.*;
	static String run() {
		return new HelloWorld().greet();
	}
};

#[test]
fn ct_java_javac_sourcepath() {
	assert_eq!(CT_JAVAC_SOURCEPATH, "Hello, World!");
}

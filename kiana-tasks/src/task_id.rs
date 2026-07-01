use rand::Rng;

const TASK_ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub fn generate_task_id(prefix: char) -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(9);
    id.push(prefix);

    for _ in 0..8 {
        let idx = rng.gen_range(0..TASK_ID_ALPHABET.len());
        id.push(TASK_ID_ALPHABET[idx] as char);
    }

    id
}

pub fn get_task_id_prefix(task_type: &str) -> char {
    match task_type {
        "local_bash" => 'b',
        "local_agent" => 'a',
        "remote_agent" => 'r',
        "in_process_teammate" => 't',
        "local_workflow" => 'w',
        "monitor_mcp" => 'm',
        "dream" => 'd',
        _ => 'x',
    }
}

pub fn create_task_id_for_type(task_type: &str) -> String {
    let prefix = get_task_id_prefix(task_type);
    generate_task_id(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_task_id() {
        let id = generate_task_id('b');
        assert_eq!(id.len(), 9);
        assert!(id.starts_with('b'));
    }

    #[test]
    fn test_task_id_prefixes() {
        assert_eq!(get_task_id_prefix("local_bash"), 'b');
        assert_eq!(get_task_id_prefix("local_agent"), 'a');
        assert_eq!(get_task_id_prefix("dream"), 'd');
        assert_eq!(get_task_id_prefix("unknown"), 'x');
    }
}
